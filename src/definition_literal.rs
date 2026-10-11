//! Literal JavaScript bodies with an explicit call stack for inductive recursion.
//! No definition image, term evaluator, proof metadata or identity dispatcher.
use ink_core::{
    definition::CheckedDefinition,
    logic::{Declaration, Term},
    LangResult,
};
use std::{collections::BTreeMap, fmt::Write};
struct Lower {
    next: usize,
    generators: std::collections::BTreeSet<String>,
}
impl Lower {
    fn expr(
        &mut self,
        t: &Term,
        env: &BTreeMap<String, String>,
        current: &str,
        out: &mut String,
    ) -> LangResult<String> {
        if self.next > 100_000 {
            return Err("literal JS emission limit".into());
        }
        let name = format!("v{}", self.next);
        self.next += 1;
        if let Term::Var(n) = t {
            return env.get(n).cloned().ok_or("literal JS variable".into());
        }
        let value = match t {
            Term::Bool(b) => b.to_string(),
            Term::U64(n) => format!("{n}n"),
            Term::Construct {
                constructor,
                arguments,
                ..
            } => {
                let xs = arguments
                    .iter()
                    .map(|a| self.expr(a, env, current, out))
                    .collect::<LangResult<Vec<_>>>()?;
                format!("__node({constructor},[{}])", xs.join(","))
            }
            Term::Call { arguments, .. } | Term::SelfCall(arguments) => {
                let id = if let Term::Call { function, .. } = t {
                    function
                } else {
                    current
                };
                let xs = arguments
                    .iter()
                    .map(|a| self.expr(a, env, current, out))
                    .collect::<LangResult<Vec<_>>>()?;
                format!(
                    "{}d_{id}({})",
                    if self.generators.contains(id) {
                        "yield "
                    } else {
                        ""
                    },
                    xs.join(",")
                )
            }
            Term::Binary { op, left, right } => {
                let a = self.expr(left, env, current, out)?;
                let b = self.expr(right, env, current, out)?;
                if let Some(operator) = op.strip_prefix("f32:") {
                    format!("__fpWord({operator:?},{a},{b})")
                } else if op == "quot" || op == "rem" {
                    format!(
                        "({b}===0n?0n:({a} {} {b}))",
                        if op == "quot" { "/" } else { "%" }
                    )
                } else if ["+", "-", "*", "&"].contains(&op.as_str()) {
                    format!("BigInt.asUintN(64,({a}){op}({b}))")
                } else {
                    let op = match op.as_str() {
                        "==" => "===",
                        "!=" => "!==",
                        x => x,
                    };
                    format!("({a}){op}({b})")
                }
            }
            Term::If {
                condition,
                on_true,
                on_false,
            } => {
                let c = self.expr(condition, env, current, out)?;
                writeln!(out, "let {name};if({c}){{").unwrap();
                let a = self.expr(on_true, env, current, out)?;
                writeln!(out, "{name}={a};}}else{{").unwrap();
                let b = self.expr(on_false, env, current, out)?;
                writeln!(out, "{name}={b};}}").unwrap();
                return Ok(name);
            }
            Term::Match {
                scrutinee,
                branches,
            } => {
                let c = self.expr(scrutinee, env, current, out)?;
                writeln!(out, "let {name};switch({c}.c){{").unwrap();
                for (i, b) in branches.iter().enumerate() {
                    let mut local = env.clone();
                    for (j, n) in b.bindings.iter().enumerate() {
                        local.insert(n.clone(), format!("{c}.a[{j}]"));
                    }
                    writeln!(out, "case {i}:{{").unwrap();
                    let x = self.expr(&b.body, &local, current, out)?;
                    writeln!(out, "{name}={x};break;}}").unwrap();
                }
                out.push_str("default:throw Error('literal constructor projection');}\n");
                return Ok(name);
            }
            Term::Var(_) => unreachable!(),
        };
        writeln!(out, "const {name}={value};").unwrap();
        Ok(name)
    }
}
pub(crate) fn lower(
    checked: &CheckedDefinition,
    queries: &BTreeMap<String, Declaration>,
) -> LangResult<String> {
    let mut ds = checked.reachable_executable_declarations()?;
    ds.extend(queries.clone());
    let exports = checked
        .exports()
        .iter()
        .chain(queries.iter().filter_map(|(id,d)| matches!(d,Declaration::Function{..}).then_some(id)))
        .collect::<Vec<_>>();
    // These constructors belong to the encapsulated representation. Literal
    // definitions never mutate them; boundary codecs construct public values.
    // Freezing each freshly owned field array added work to every rebuilt path.
    let mut out=String::from("const __node=(c,a)=>({c,a});\nfunction __run(call){const stack=[call];let result;while(stack.length){const frame=stack[stack.length-1],step=frame.next(result);if(step.done){result=step.value;stack.pop();}else{result=undefined;stack.push(step.value);}}return result;}\n");
    fn calls(t: &Term, ids: &mut std::collections::BTreeSet<String>) {
        match t {
            Term::Call {
                function,
                arguments,
            } => {
                ids.insert(function.clone());
                for a in arguments {
                    calls(a, ids);
                }
            }
            Term::Construct { arguments, .. } | Term::SelfCall(arguments) => {
                for a in arguments {
                    calls(a, ids);
                }
            }
            Term::Binary { left, right, .. } => {
                calls(left, ids);
                calls(right, ids);
            }
            Term::If {
                condition,
                on_true,
                on_false,
            } => {
                calls(condition, ids);
                calls(on_true, ids);
                calls(on_false, ids);
            }
            Term::Match {
                scrutinee,
                branches,
            } => {
                calls(scrutinee, ids);
                for b in branches {
                    calls(&b.body, ids);
                }
            }
            _ => (),
        }
    }
    let mut generators = ds
        .iter()
        .filter_map(|(id, d)| {
            let Declaration::Function { body, .. } = d else { return None };
            // A structural annotation permits recursion; only a self-call
            // needs suspension. Propagating unused annotations turned ordinary
            // projections and their callers into generator frames.
            let mut recursive = false;
            ink_core::logic::walk_term(body, |t| {
                recursive |= matches!(t, Term::SelfCall(_));
            });
            recursive.then_some(id.clone())
        })
        .collect::<std::collections::BTreeSet<_>>();
    loop {
        let mut changed = false;
        for (id, d) in &ds {
            if let Declaration::Function { body, .. } = d {
                let mut ids = std::collections::BTreeSet::new();
                calls(body, &mut ids);
                if ids.iter().any(|x| generators.contains(x)) {
                    changed |= generators.insert(id.clone());
                }
            }
        }
        if !changed {
            break;
        }
    }
    let mut lower = Lower {
        next: 0,
        generators,
    };
    out.push_str(include_str!("definition-f32.mjs.txt"));
    for (id, d) in &ds {
        if let Declaration::Function { params, body, .. } = d {
            let env = params
                .iter()
                .enumerate()
                .map(|(i, (n, _))| (n.clone(), format!("a{i}")))
                .collect();
            let inputs = (0..params.len())
                .map(|i| format!("a{i}"))
                .collect::<Vec<_>>()
                .join(",");
            let mut code = String::new();
            let result = lower.expr(body, &env, id, &mut code)?;
            writeln!(
                out,
                "function{} d_{id}({inputs}){{{code}return {result};}}",
                if lower.generators.contains(id) {
                    "*"
                } else {
                    ""
                }
            )
            .unwrap();
        }
    }
    for id in exports {
        let target = checked.choices().get(id).unwrap_or(id);
        let Declaration::Function { params, .. } = &ds[target] else {
            return Err("literal JS export".into());
        };
        let args = (0..params.len())
            .map(|i| format!("a{i}"))
            .collect::<Vec<_>>()
            .join(",");
        writeln!(
            out,
            "function f_{id}({args}){{return {};}}",
            if lower.generators.contains(target) {
                format!("__run(d_{target}({args}))")
            } else {
                format!("d_{target}({args})")
            }
        )
        .unwrap();
    }
    if out.len() > 16_000_000 {
        return Err("literal JS source limit".into());
    }
    Ok(out)
}
