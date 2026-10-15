//! Literal residual functions over sealed immutable inputs. Dependencies and
//! readonly loans use core structural analyses, never an algorithm recogniser.
use super::{Emitter, Env};
use ink_core::{analysis, local_collections, recursion, syntax::*, LangResult};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

type Dependencies = BTreeMap<String, Option<BTreeSet<usize>>>;

/// Iteration arguments change on every iteration, even when the enclosing
/// collection is fixed. Preserve only lexical captures' original provenance.
fn callback<'a>(expression: &'a Expr, env: &Env, deps: &Dependencies,
    params: &[(String, Type)]) -> LangResult<(&'a Expr, Env, Dependencies)> {
    let (body, local) = Emitter::callback(expression, env, params)?;
    let mut found = deps.clone();
    let mut binder = expression;
    for _ in params {
        let Expr::Lambda(name, rest) = binder else { return Err("prepared callback binding".into()) };
        found.insert(name.clone(), None);
        binder = rest;
    }
    Ok((body, local, found))
}

fn shared_callback(expression: &Expr, env: &Env, p: &Program) -> bool {
    // The optional lane does not lend new cross-invocation caches to captured
    // mutable owners. Ordinary emission retains the existing loan analysis.
    analysis::free_variables(expression).iter().all(|n|
        env.get(n).is_none_or(|(_, ty)| !local_collections::owned(ty, p)))
}

/// A bounded search for potentially reusable work under a parameter mask.
/// This only avoids empty residual wrappers; it grants no transformation facts.
/// Unknown/deep calls retain ordinary execution. Iteration bindings are never
/// stable merely because their collection or enclosing loop is stable.
fn potential(expression: &Expr, env: &BTreeMap<String, bool>, p: &Program) -> bool {
    type Stable = BTreeMap<String, bool>;
    fn stable(e: &Expr, env: &Stable, p: &Program) -> bool {
        analysis::free_variables(e).iter().all(|n| env.get(n).copied().unwrap_or_else(||
            matches!(n.as_str(), "None" | "Map" | "Buffer") || p.enums.contains_key(n)))
    }
    fn callback(e: &Expr, env: &Stable, count: usize, p: &Program, work: &mut usize, depth: usize) -> bool {
        let mut env = env.clone(); let mut body = e;
        for _ in 0..count {
            let Expr::Lambda(name, rest) = body else { return false };
            env.insert(name.clone(), false); body = rest;
        }
        visit(body, &env, p, work, depth + 1)
    }
    fn visit(e: &Expr, env: &Stable, p: &Program, work: &mut usize, depth: usize) -> bool {
        if *work == 0 || depth >= 32 { return false; } *work -= 1;
        match e {
            Expr::Let(name, _, value, rest) => {
                let fixed = stable(value, env, p);
                if fixed && !matches!(value.as_ref(), Expr::Var(_) | Expr::Num(_) | Expr::Float(_) | Expr::Double(_) | Expr::Bool(_) | Expr::String(_) | Expr::Unit) { return true; }
                if visit(value, env, p, work, depth + 1) { return true; }
                let mut env = env.clone(); env.insert(name.clone(), fixed);
                visit(rest, &env, p, work, depth + 1)
            }
            Expr::Call(name, args) if name == "choose" && args.len() == 3 => {
                let mut expensive = false;
                analysis::walk(&args[0], |e| expensive |= match e {
                    Expr::Call(name, _) => p.functions.iter().any(|f| &f.name == name)
                        || matches!(name.as_str(), "foldl" | "foldr" | "fold_until" | "repeat_until" | "sum"),
                    Expr::Method(_, method, _) => matches!(method.as_str(), "map" | "filter" | "zip" | "scan"),
                    _ => false,
                });
                (expensive && (stable(&args[0], env, p) || visit(&args[0], env, p, work, depth + 1)))
                    || args[1..].iter().any(|e| visit(e, env, p, work, depth + 1))
            }
            Expr::Call(name, args) if name == "repeat_until" && args.len() == 4 => {
                callback(&args[2], env, 1, p, work, depth) || callback(&args[3], env, 2, p, work, depth)
            }
            Expr::Call(name, args) if matches!(name.as_str(), "foldl" | "foldr" | "fold_until") => {
                args[2..].iter().any(|e| callback(e, env, 2, p, work, depth))
            }
            Expr::Call(name, args) => {
                let Some(f) = p.functions.iter().find(|f| &f.name == name) else {
                    return (matches!(name.as_str(), "list" | "Some" | "Ok" | "Err")
                        || ink_core::primitives::primitive(name) || ink_core::word::primitive(name))
                        && args.iter().any(|e| visit(e, env, p, work, depth + 1));
                };
                if recursion::recursive(f) { return false; }
                let local = f.params.iter().zip(args).map(|((n, _), arg)| (n.clone(), stable(arg, env, p))).collect();
                visit(&f.body, &local, p, work, depth + 1)
                    || args.iter().any(|e| visit(e, env, p, work, depth + 1))
            }
            Expr::Method(receiver, method, args) if matches!(method.as_str(), "map" | "map_indexed" | "filter") && args.len() == 1 => {
                callback(&args[0], env, if method == "map_indexed" {2} else {1}, p, work, depth)
                    || visit(receiver, env, p, work, depth + 1)
            }
            Expr::Method(receiver, method, args) if method == "concat" => visit(receiver, env, p, work, depth + 1)
                || args.iter().any(|e| visit(e, env, p, work, depth + 1)),
            Expr::Method(receiver, _, _) if matches!(receiver.as_ref(), Expr::Call(..) | Expr::Let(..)) => visit(receiver, env, p, work, depth + 1),
            Expr::Binary(_, a, b) => visit(a, env, p, work, depth + 1) || visit(b, env, p, work, depth + 1),
            Expr::Neg(a) | Expr::Field(a, _) => visit(a, env, p, work, depth + 1),
            Expr::Record(_, fields) => fields.iter().any(|(_, e)| visit(e, env, p, work, depth + 1)),
            _ => false,
        }
    }
    visit(expression, env, p, &mut 512, 0)
}
fn useful(f: &Function, fixed: &[bool], p: &Program) -> bool {
    let env = f.params.iter().enumerate().map(|(i, (n, _))| (n.clone(), fixed.get(i).copied().unwrap_or(false))).collect();
    potential(&f.body, &env, p)
}

#[derive(Default)]
struct Preparation {
    declarations: String,
    retained: usize,
    count: usize,
    delegated: usize,
}

fn dependencies(expression: &Expr, env: &Dependencies, p: &Program) -> Option<BTreeSet<usize>> {
    analysis::free_variables(expression).iter().try_fold(BTreeSet::new(), |mut out, name| {
        if let Some(Some(found)) = env.get(name) { out.extend(found); Some(out) }
        else if matches!(name.as_str(), "None" | "Map" | "Buffer") || p.enums.contains_key(name) { Some(out) }
        else { None }
    })
}

fn ordinary(emitter: &mut Emitter<'_>, expression: &Expr, env: &Env, ty: &Type) -> LangResult<String> {
    let params = env.iter().map(|(n, (_, t))| (n.clone(), t.clone())).collect::<Vec<_>>();
    // Every residual cut treats previous values as shared, even when they came
    // from a fresh allocation in a preceding source call.
    emitter.in_place = ink_core::in_place::analyse_with_result(expression, &params, Some(ty), emitter.p);
    emitter.expr(expression, env, Some(ty))
}

#[allow(clippy::too_many_arguments)]
fn binding(emitter: &mut Emitter<'_>, expression: &Expr, env: &Env, deps: &Dependencies,
    ty: &Type, readonly: bool, plan: &mut Preparation, depth: usize) -> LangResult<String> {
    plan.count += 1;
    if plan.count > 64 { return Err("prepared residual binding budget".into()); }
    let var = emitter.fresh();
    if let Some(found) = dependencies(expression, deps, emitter.p).filter(|_| readonly) {
        let cache = emitter.fresh(); let ready = emitter.fresh(); let eligible = emitter.fresh();
        let test = found.iter().map(|i| format!("fixed[{i}]"))
            .collect::<Vec<_>>().join("&&");
        plan.declarations.push_str(&format!("const {eligible}={};let {ready}=false,{cache};\n", if test.is_empty() {"true"} else {&test}));
        emitter.line(format!("let {var};if({eligible}&&{ready}){{{var}={cache};}}else{{"));
        let value = residual(emitter, expression, env.clone(), deps.clone(), ty, plan, depth + 1)?;
        emitter.line(format!("{var}={value};if({eligible}){{{cache}={var};{ready}=true;}}}}"));
        plan.retained += 1;
    } else {
        let value = residual(emitter, expression, env.clone(), deps.clone(), ty, plan, depth + 1)?;
        emitter.line(format!("const {var}={value};"));
    }
    Ok(var)
}

#[allow(clippy::too_many_arguments)]
fn operand(emitter: &mut Emitter<'_>, expression: &Expr, env: &mut Env, deps: &mut Dependencies,
    ty: &Type, plan: &mut Preparation, depth: usize) -> LangResult<Expr> {
    let found = dependencies(expression, deps, emitter.p);
    let value = residual(emitter, expression, env.clone(), deps.clone(), ty, plan, depth + 1)?;
    let value = emitter.bind(&value);
    let name = loop {
        let n = emitter.fresh();
        if !env.contains_key(&n) && !emitter.p.enums.contains_key(&n) && !emitter.p.ids.contains(&n)
            && !emitter.p.states.iter().any(|s| s.name == n) && !emitter.p.keeps.iter().any(|s| s.name == n) { break n; }
    };
    env.insert(name.clone(), (value, ty.clone())); deps.insert(name.clone(), found);
    Ok(Expr::Var(name))
}

/// Enter strict subexpressions without lifting lazy operands or callbacks.
/// Reuse the ordinary emitter for the final operation and its exact carriers.
fn strict(emitter: &mut Emitter<'_>, expression: &Expr, mut env: Env, mut deps: Dependencies,
    result: &Type, plan: &mut Preparation, depth: usize) -> LangResult<String> {
    let rebound = match expression {
        Expr::Binary(op, a, b) if matches!(op.as_str(), "&&" | "||") => {
            let args = if op == "&&" { vec![*a.clone(), *b.clone(), Expr::Bool(false)] }
                else { vec![*a.clone(), Expr::Bool(true), *b.clone()] };
            return residual(emitter, &Expr::Call("choose".into(), args), env, deps, result, plan, depth + 1);
        }
        Expr::Binary(op, a, b) => {
            let ts = Emitter::types(&env);
            let context = if ink_core::check::is_numeric_value(result) {Some(result.clone())}
                else {ink_core::check::hint(a, &ts, emitter.p).or_else(|| ink_core::check::hint(b, &ts, emitter.p))};
            let ty = emitter.infer(a, &env, context.as_ref())?;
            let a = operand(emitter, a, &mut env, &mut deps, &ty, plan, depth)?;
            let b = operand(emitter, b, &mut env, &mut deps, &ty, plan, depth)?;
            Expr::Binary(op.clone(), Box::new(a), Box::new(b))
        }
        Expr::Neg(a) => Expr::Neg(Box::new(operand(emitter, a, &mut env, &mut deps, result, plan, depth)?)),
        Expr::Field(a, field) => {
            let ty = emitter.infer(a, &env, None)?;
            Expr::Field(Box::new(operand(emitter, a, &mut env, &mut deps, &ty, plan, depth)?), field.clone())
        }
        Expr::Record(name, fields) => {
            let types = emitter.p.records[name].clone(); let mut next = Vec::new();
            for (name, e) in fields {
                let ty = &types.iter().find(|(n, _)| n == name).ok_or("prepared record field")?.1;
                next.push((name.clone(), operand(emitter, e, &mut env, &mut deps, ty, plan, depth)?));
            }
            Expr::Record(name.clone(), next)
        }
        Expr::Call(name, args) => {
            let params = if let Some(f) = emitter.p.functions.iter().find(|f| &f.name == name) {
                f.params.iter().map(|(_, t)| t.clone()).collect()
            } else if ink_core::primitives::primitive(name) {
                let first = ink_core::primitives::input_type(name, args.first(), |a| emitter.infer(a, &env, None))?;
                ink_core::primitives::signature(name, first.as_ref())?.0
            } else if ink_core::word::primitive(name) {ink_core::word::arguments(name, result)}
            else {match (name.as_str(), result) {
                ("list", Type::List(t)) => vec![*t.clone(); args.len()],
                ("Some", Type::Option(t)) | ("Ok", Type::Result(t, _)) | ("Err", Type::Result(_, t)) => vec![*t.clone()],
                _ => return ordinary(emitter, expression, &env, result),
            }};
            let mut next = Vec::new();
            for (e, ty) in args.iter().zip(params) { next.push(operand(emitter, e, &mut env, &mut deps, &ty, plan, depth)?); }
            Expr::Call(name.clone(), next)
        }
        Expr::Method(xs, method, args) if method == "concat" && matches!(result, Type::List(_)) => {
            let xs = operand(emitter, xs, &mut env, &mut deps, result, plan, depth)?;
            let mut next = Vec::new();
            for e in args {next.push(operand(emitter, e, &mut env, &mut deps, result, plan, depth)?);}
            Expr::Method(Box::new(xs), method.clone(), next)
        }
        _ => return ordinary(emitter, expression, &env, result),
    };
    ordinary(emitter, &rebound, &env, result)
}

fn factory(emitter: &mut Emitter<'_>, f: &Function, index: usize) -> LangResult<(String, usize, usize)> {
    if recursion::recursive(f) || local_collections::owned(&f.result, emitter.p)
        || f.params.iter().any(|(_, t)| local_collections::owned(t, emitter.p))
        || analysis::node_count(&f.body, 4096).is_none() {
        return Err("prepared residual requires a bounded pure value entry".into());
    }
    let mut env = Env::new();
    let mut dependencies = BTreeMap::<String, Option<BTreeSet<usize>>>::new();
    let mut vars = Vec::new();
    for (i, (name, ty)) in f.params.iter().enumerate() {
        let var = emitter.fresh(); vars.push(var.clone());
        env.insert(name.clone(), (var, ty.clone()));
        dependencies.insert(name.clone(), Some([i].into_iter().collect()));
    }
    let mut plan = Preparation::default();
    emitter.lines.clear();
    let value = residual(emitter, &f.body, env, dependencies, &f.result,
        &mut plan, 0)?;
    if plan.retained == 0 && plan.delegated == 0 { return Err("no reusable lexical bindings".into()); }
    let declarations = plan.declarations;
    let source = format!("function __prepareFn{index}(fixed,depth=0){{if(depth>=16)return __fn{index};{declarations}return function({}){{{}return {value};}};}}\n", vars.join(","), emitter.lines);
    if source.len() > 131_072 { return Err("prepared residual source budget".into()); }
    Ok((source, plan.retained, plan.delegated))
}

fn residual(emitter: &mut Emitter<'_>, body: &Expr, mut env: Env,
    mut deps: Dependencies, result: &Type, plan: &mut Preparation, depth: usize,
) -> LangResult<String> {
    if depth >= 48 { return Err("prepared residual depth budget".into()); }
    if emitter.budget == 0 { return Err("prepared residual work budget".into()); }
    emitter.budget -= 1;
    if let Expr::Let(name, annotation, initializer, rest) = body {
        let ty = emitter.infer(initializer, &env, annotation.as_deref())?;
        let found = dependencies(initializer, &deps, emitter.p);
        let ts = Emitter::types(&env);
        let readonly = !local_collections::owned(&ty, emitter.p) || {
            let callback = Expr::Lambda(name.clone(), rest.clone());
            local_collections::callback_parameter_loans(&callback, &ts, emitter.p, &[ty.clone()], result)
                .is_ok_and(|loans| loans == [true])
        };
        let var = binding(emitter, initializer, &env, &deps, &ty, readonly, plan, depth)?;
        env.insert(name.clone(), (var, ty)); deps.insert(name.clone(), found);
        return residual(emitter, rest, env, deps, result, plan, depth + 1);
    }
    if let Expr::Call(name, args) = body {
        if name == "repeat_until" && args.len() == 4
            && !local_collections::owned(result, emitter.p)
            && args[2..].iter().all(|e| shared_callback(e, &env, emitter.p)) {
            let limit = ordinary(emitter, &args[0], &env, &Type::U32)?;
            let limit = emitter.bind(&limit);
            let initial = ordinary(emitter, &args[1], &env, result)?;
            let split = emitter.split_fields(result);
            let out = match &split {
                Some(fields) => emitter.split_state(&initial, fields),
                None => {
                    let out = emitter.fresh(); emitter.line(format!("let {out}={initial};")); out
                }
            };
            let index = emitter.fresh();
            emitter.line(format!("for(let {index}=0;{index}<{limit};{index}++){{"));
            let (stop, local, found) = callback(&args[2], &env, &deps, &[(out.clone(), result.clone())])?;
            let stop = residual(emitter, stop, local, found, &Type::Bool, plan, depth + 1)?;
            emitter.line(format!("if({stop})break;"));
            let idx = emitter.bind(&format!("({index}>>>0)"));
            let (step, local, found) = callback(&args[3], &env, &deps,
                &[(idx, Type::U32), (out.clone(), result.clone())])?;
            let value = residual(emitter, step, local, found, result, plan, depth + 1)?;
            if split.is_some() {
                emitter.split_update(&out, &value); emitter.line("}");
                let value = emitter.rebuild(&out); emitter.split.remove(&out); return Ok(value);
            }
            emitter.line(format!("{out}={value};}}")); return Ok(out);
        }
        if matches!(name.as_str(), "foldl" | "foldr" | "fold_until")
            && !local_collections::owned(result, emitter.p)
            && args[2..].iter().all(|e| shared_callback(e, &env, emitter.p)) {
            let Type::List(inner) = emitter.infer(&args[0], &env, None)? else { return Err("prepared fold input".into()) };
            let xs = ordinary(emitter, &args[0], &env, &Type::List(inner.clone()))?;
            let xs = emitter.bind(&xs);
            let initial = ordinary(emitter, &args[1], &env, result)?;
            let out = emitter.fresh(); emitter.line(format!("let {out}={initial};"));
            let index = emitter.fresh();
            emitter.line(if name == "foldr" {format!("for(let {index}={xs}.length-1;{index}>=0;{index}--){{")}
                else {format!("for(let {index}=0;{index}<{xs}.length;{index}++){{")});
            let element = emitter.bind(&super::word_load(&format!("{xs}[{index}]"), &inner));
            let params = [(element, *inner), (out.clone(), result.clone())];
            let stop = if name == "fold_until" {
                let (stop, local, found) = callback(&args[2], &env, &deps, &params)?;
                let stop = residual(emitter, stop, local, found, &Type::Bool, plan, depth + 1)?;
                Some(emitter.bind(&stop))
            } else { None };
            let (step, local, found) = callback(&args[if stop.is_some() {3} else {2}], &env, &deps, &params)?;
            let value = residual(emitter, step, local, found, result, plan, depth + 1)?;
            emitter.line(format!("{out}={value};"));
            if let Some(stop) = stop { emitter.line(format!("if({stop})break;")); }
            emitter.line("}"); return Ok(out);
        }
        if name == "choose" && args.len() == 3 {
            // The condition executes at its original source position. Stable
            // successful checks may persist; dynamic guards and unvisited
            // branches remain dynamic/lazy, and thrown failures are retried.
            let mut expensive = false;
            analysis::walk(&args[0], |e| expensive |= match e {
                Expr::Call(name, _) => emitter.p.functions.iter().any(|f| &f.name == name)
                    || matches!(name.as_str(), "foldl" | "foldr" | "fold_until" | "repeat_until" | "sum"),
                Expr::Method(_, method, _) => matches!(method.as_str(), "map" | "filter" | "zip" | "scan"),
                _ => false,
            });
            let condition = if expensive {
                binding(emitter, &args[0], &env, &deps, &Type::Bool, true, plan, depth)?
            } else {
                let value = ordinary(emitter, &args[0], &env, &Type::Bool)?;
                emitter.bind(&value)
            };
            let out = emitter.fresh();
            emitter.line(format!("let {out};if({condition}){{"));
            let yes = residual(emitter, &args[1], env.clone(), deps.clone(), result, plan, depth + 1)?;
            emitter.line(format!("{out}={yes};}}else{{"));
            let no = residual(emitter, &args[2], env, deps, result, plan, depth + 1)?;
            emitter.line(format!("{out}={no};}}")); return Ok(out);
        }
        if let Some((index, f)) = emitter.p.functions.iter().enumerate().find(|(_, f)| &f.name == name) {
            // Keep helper boundaries instead of copying a large call closure
            // into every entry. Each syntactic site lazily creates a distinct
            // residual helper. The mask carries transitive parameter provenance,
            // never an identity/trust claim about its runtime values.
            let possible = args.iter().map(|arg| dependencies(arg, &deps, emitter.p).is_some()).collect::<Vec<_>>();
            if !recursion::recursive(f) && useful(f, &possible, emitter.p) && !local_collections::owned(&f.result, emitter.p)
                && !f.params.iter().any(|(_, t)| local_collections::owned(t, emitter.p)) {
                let f = f.clone(); let mut actuals = Vec::new(); let mut fixed = Vec::new();
                for (arg, (_, ty)) in args.iter().zip(&f.params) {
                    // Actual arguments still execute once, left to right, before
                    // any work in the callee (including its first cached check).
                    let value = residual(emitter, arg, env.clone(), deps.clone(), ty, plan, depth + 1)?;
                    actuals.push(emitter.bind(&value));
                    fixed.push(match dependencies(arg, &deps, emitter.p) {
                        Some(found) if found.is_empty() => "true".into(),
                        Some(found) => found.iter().map(|i| format!("fixed[{i}]")).collect::<Vec<_>>().join("&&"),
                        None => "false".into(),
                    });
                }
                let helper = emitter.fresh(); let factory = emitter.fresh();
                plan.declarations.push_str(&format!("let {helper};\n"));
                emitter.line(format!("if(!{helper}){{const {factory}=__preparedFactories.get({index});{helper}={factory}?{factory}([{}],depth+1):__fn{index};}}",fixed.join(",")));
                plan.delegated += 1;
                return Ok(format!("{helper}({})", actuals.join(",")));
            }
        }
    }
    if let Expr::Method(receiver, method, args) = body {
        if matches!(method.as_str(), "map" | "map_indexed" | "filter") && args.len() == 1
            && matches!(result, Type::List(_)) && !local_collections::owned(result, emitter.p)
            && shared_callback(&args[0], &env, emitter.p) {
            if let Type::List(inner) = emitter.infer(receiver, &env, None)? {
                let xs = ordinary(emitter, receiver, &env, &Type::List(inner.clone()))?;
                let xs = emitter.bind(&xs); let out = emitter.fresh(); let index = emitter.fresh();
                emitter.line(format!("const {out}=[];for(let {index}=0;{index}<{xs}.length;{index}++){{"));
                let element = emitter.bind(&super::word_load(&format!("{xs}[{index}]"), &inner));
                let params = if method == "map_indexed" {
                    vec![(format!("({index}>>>0)"), Type::U32), (element.clone(), *inner.clone())]
                } else { vec![(element.clone(), *inner.clone())] };
                let (body, local, found) = callback(&args[0], &env, &deps, &params)?;
                let Type::List(output) = result else { unreachable!() };
                let ty = if method == "filter" { &Type::Bool } else { output.as_ref() };
                let value = residual(emitter, body, local, found, ty, plan, depth + 1)?;
                if method == "filter" { emitter.line(format!("if({value}){out}.push({element});}}")); }
                else { emitter.line(format!("{out}.push({value});}}")); }
                return Ok(out);
            }
        }
        if let Ok(ty) = emitter.infer(receiver, &env, None) {
            if matches!(ty, Type::Option(_) | Type::Result(..)) && !local_collections::owned(&ty, emitter.p)
                && matches!(receiver.as_ref(), Expr::Call(..) | Expr::Let(..)) {
                // Preserve the ordinary sum method and its lazy callback. Only its
                // already-strict receiver enters the bounded residual traversal.
                let value = residual(emitter, receiver, env.clone(), deps.clone(), &ty, plan, depth + 1)?;
                let var = emitter.bind(&value);
                let name = loop { let name = emitter.fresh(); if !env.contains_key(&name) { break name; } };
                env.insert(name.clone(), (var, ty));
                let expression = Expr::Method(Box::new(Expr::Var(name)), method.clone(), args.clone());
                return ordinary(emitter, &expression, &env, result);
            }
        }
    }
    if potential(body, &deps.iter().map(|(n, d)| (n.clone(), d.is_some())).collect(), emitter.p) {
        strict(emitter, body, env, deps, result, plan, depth)
    } else { ordinary(emitter, body, &env, result) }
}

pub(super) fn emit(emitter: &mut Emitter<'_>) -> (String, Vec<Value>) {
    // An optional lane has independent limits; it cannot consume the ordinary
    // emitter's work budget or reject an otherwise supported checked program.
    emitter.budget = 200_000;
    emitter.depth = 0;
    let mut source = String::new(); let mut entries = Vec::new(); let mut metadata = Vec::new();
    for (index, f) in emitter.p.functions.iter().enumerate() {
        let callbacks = emitter.callbacks.len(); let equality = emitter.equality.clone();
        let budget = emitter.budget;
        emitter.split.clear(); emitter.lines.clear();
        match factory(emitter, f, index) {
            Ok((code, retained, delegated)) if source.len() + code.len() <= 262_144 => {
                source.push_str(&code); entries.push(format!("[{index},__prepareFn{index}]"));
                metadata.push(json!({"function":f.name,"retained_bindings":retained,"delegated_calls":delegated,"factory":true}));
            }
            result => {
                emitter.callbacks.truncate(callbacks); emitter.equality = equality; emitter.budget = budget;
                let reason = match result { Err(e) => e, _ => "prepared module source budget".into() };
                metadata.push(json!({"function":f.name,"factory":false,"reason":reason}));
            }
        }
        emitter.depth = 0;
    }
    source.push_str(&format!("const __preparedFactories=new Map([{}]);\n", entries.join(",")));
    (source, metadata)
}
