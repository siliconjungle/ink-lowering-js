//! Literal residual functions over sealed immutable inputs. Dependencies and
//! readonly loans use core structural analyses, never an algorithm recogniser.
use super::{Emitter, Env};
use ink_core::{analysis, local_collections, recursion, syntax::*, LangResult};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

type Dependencies = BTreeMap<String, Option<BTreeSet<usize>>>;

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
            if !recursion::recursive(f) && !local_collections::owned(&f.result, emitter.p)
                && !f.params.iter().any(|(_, t)| local_collections::owned(t, emitter.p)) {
                let f = f.clone(); let mut actuals = Vec::new(); let mut fixed = Vec::new();
                for (arg, (_, ty)) in args.iter().zip(&f.params) {
                    // Actual arguments still execute once, left to right, before
                    // any work in the callee (including its first cached check).
                    let value = ordinary(emitter, arg, &env, ty)?;
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
    ordinary(emitter, body, &env, result)
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
