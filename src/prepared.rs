//! Literal residual functions over sealed immutable inputs. Dependencies and
//! readonly loans use core structural analyses, never an algorithm recogniser.
use super::{Emitter, Env};
use ink_core::{analysis, local_collections, recursion, syntax::*, LangResult};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

fn factory(emitter: &mut Emitter<'_>, f: &Function, index: usize) -> LangResult<(String, usize)> {
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
    let mut declarations = String::new();
    let mut retained = 0;
    let mut count = 0;
    emitter.lines.clear();
    let value = residual(emitter, &f.body, env, dependencies, &f.result,
        &mut declarations, &mut retained, &mut count, 0)?;
    if retained == 0 { return Err("no reusable lexical bindings".into()); }
    let source = format!("function __prepareFn{index}(fixed){{{declarations}return function({}){{{}return {value};}};}}\n", vars.join(","), emitter.lines);
    if source.len() > 131_072 { return Err("prepared residual source budget".into()); }
    Ok((source, retained))
}

#[allow(clippy::too_many_arguments)]
fn residual(emitter: &mut Emitter<'_>, body: &Expr, mut env: Env,
    mut dependencies: BTreeMap<String, Option<BTreeSet<usize>>>, result: &Type,
    declarations: &mut String, retained: &mut usize, count: &mut usize, depth: usize,
) -> LangResult<String> {
    if depth >= 48 { return Err("prepared residual depth budget".into()); }
    if let Expr::Let(name, annotation, initializer, rest) = body {
        *count += 1;
        if *count > 64 { return Err("prepared residual binding budget".into()); }
        if emitter.budget == 0 { return Err("prepared residual work budget".into()); }
        emitter.budget -= 1;
        let ty = emitter.infer(initializer, &env, annotation.as_deref())?;
        let deps = analysis::free_variables(initializer).iter().try_fold(BTreeSet::new(), |mut out, name| {
            if let Some(Some(found)) = dependencies.get(name) { out.extend(found); Some(out) }
            else if matches!(name.as_str(), "None" | "Map" | "Buffer") || emitter.p.enums.contains_key(name) { Some(out) }
            else { None }
        });
        let ts = Emitter::types(&env);
        let readonly = !local_collections::owned(&ty, emitter.p) || {
            let callback = Expr::Lambda(name.clone(), rest.clone());
            local_collections::callback_parameter_loans(&callback, &ts, emitter.p, &[ty.clone()], result)
                .is_ok_and(|loans| loans == [true])
        };
        let var = emitter.fresh();
        // Bindings at this cut are shared parameters, never uniquely owned
        // allocations. Internal fresh values still get ordinary lowering.
        let params = env.iter().map(|(n, (_, t))| (n.clone(), t.clone())).collect::<Vec<_>>();
        emitter.in_place = ink_core::in_place::analyse_with_result(initializer, &params, Some(&ty), emitter.p);
        if readonly && deps.is_some() {
            let cache = emitter.fresh(); let ready = emitter.fresh(); let eligible = emitter.fresh();
            let test = deps.as_ref().unwrap().iter().map(|i| format!("fixed[{i}]"))
                .collect::<Vec<_>>().join("&&");
            declarations.push_str(&format!("const {eligible}={};let {ready}=false,{cache};\n", if test.is_empty() {"true"} else {&test}));
            emitter.line(format!("let {var};if({eligible}&&{ready}){{{var}={cache};}}else{{"));
            let value = emitter.expr(initializer, &env, Some(&ty))?;
            emitter.line(format!("{var}={value};if({eligible}){{{cache}={var};{ready}=true;}}}}"));
            *retained += 1;
        } else {
            let value = emitter.expr(initializer, &env, Some(&ty))?;
            emitter.line(format!("const {var}={value};"));
        }
        env.insert(name.clone(), (var, ty)); dependencies.insert(name.clone(), deps);
        return residual(emitter, rest, env, dependencies, result, declarations, retained, count, depth + 1);
    }
    if let Expr::Call(name, args) = body {
        if name == "choose" && args.len() == 3 {
            let condition = emitter.expr(&args[0], &env, Some(&Type::Bool))?;
            let condition = emitter.bind(&condition); let out = emitter.fresh();
            emitter.line(format!("let {out};if({condition}){{"));
            let yes = residual(emitter, &args[1], env.clone(), dependencies.clone(), result, declarations, retained, count, depth + 1)?;
            emitter.line(format!("{out}={yes};}}else{{"));
            let no = residual(emitter, &args[2], env, dependencies, result, declarations, retained, count, depth + 1)?;
            emitter.line(format!("{out}={no};}}")); return Ok(out);
        }
    }
    let params = env.iter().map(|(n, (_, t))| (n.clone(), t.clone())).collect::<Vec<_>>();
    emitter.in_place = ink_core::in_place::analyse_with_result(body, &params, Some(result), emitter.p);
    emitter.expr(body, &env, Some(result))
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
            Ok((code, retained)) if source.len() + code.len() <= 262_144 => {
                source.push_str(&code); entries.push(format!("[{index},__prepareFn{index}]"));
                metadata.push(json!({"function":f.name,"retained_bindings":retained,"factory":true}));
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
