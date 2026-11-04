//! Compiled tasks: each `task` becomes an async JavaScript function printed
//! from its mid-level IR graph (see ink-lang docs/effect-tasks.md and
//! docs/mid-level-ir.md).
//!
//! Every top-level node of the task graph becomes a promise that awaits
//! exactly the nodes it depends on (`inputs` and `after`), so independent
//! effects overlap and queries, changes and ordered effects keep the order the
//! compiler fixed. Step expressions and fallbacks print as plain JavaScript;
//! effect replies still enter Ink through the generated accept query, which
//! checks the declared type and `ensures`. The earliest failure in source order
//! decides the result; steps after it do not start, and requests already in
//! flight are aborted and recorded as discarded. Results and traces equal the interpreted
//! host's (`runtime/hosts/tasks.mjs`), so `replayTask` and `ink replay-task`
//! check compiled runs too.
use crate::mir::{name, Printer, HELPERS};
use ink_core::{
    core::CheckedModule,
    mir::{lower, Graph, Kind, Node, Op, Region},
    tasks::Kind as StepKind,
    LangResult,
};
use std::collections::{BTreeMap, BTreeSet};

const RUNTIME: &str = r#"class TaskFailed extends Error { constructor(message, trace) { super(message); this.name = 'TaskFailed'; this.trace = trace; } }
const __skip = Symbol('skip');
const __isErr = v => v !== null && typeof v === 'object' && Object.hasOwn(v, 'Err');
const __caller = state => typeof state === 'function' ? state : (n, a) => state.invoke(n, a).result;
function __limit(n) { let active = 0; const queue = []; return async f => { if (active >= n) await new Promise(r => queue.push(r)); active++; try { return await f(); } finally { active--; queue.shift()?.(); } }; }
"#;

/// External references of a node: everything it reads that its own regions
/// do not define.
fn externals(n: &Node, out: &mut BTreeSet<u32>) {
    fn region(r: &Region, defined: &mut BTreeSet<u32>, out: &mut BTreeSet<u32>) {
        for n in &r.nodes {
            for d in n.inputs.iter().chain(&n.after) {
                if !defined.contains(d) {
                    out.insert(*d);
                }
            }
            for inner in &n.regions {
                region(inner, defined, out);
            }
            defined.insert(n.id);
        }
        if let Some(x) = r.result {
            if !defined.contains(&x) {
                out.insert(x);
            }
        }
    }
    out.extend(n.inputs.iter().chain(&n.after));
    let mut defined = BTreeSet::new();
    for r in &n.regions {
        region(r, &mut defined, out);
    }
}

fn task(g: &Graph, module: &CheckedModule, graphs: &[Graph]) -> LangResult<String> {
    let p = module.program();
    let t = p.contracts.tasks.tasks.iter().find(|t| t.name == g.name).ok_or("tasks: manifest entry missing")?;
    if g.steps.len() != t.steps.len() {
        return Err(format!("tasks: {} graph and manifest disagree", t.name));
    }
    let main: BTreeMap<u32, usize> = g.steps.iter().enumerate().map(|(i, s)| (s.node, i)).collect();
    let tries: BTreeMap<u32, usize> = g.steps.iter().enumerate().filter(|(_, s)| s.value != s.node).map(|(i, s)| (s.value, i)).collect();
    // Value nodes of bound names, for the accept query's `uses`.
    let mut bound: BTreeMap<String, u32> = BTreeMap::new();
    let params: Vec<u32> = g.body.nodes.iter().filter(|n| matches!(n.op, Op::Param { .. })).map(|n| n.id).collect();
    for ((n, _), id) in t.params.iter().zip(&params) {
        bound.insert(n.clone(), *id);
    }
    let mut uses_of = BTreeMap::new();
    for (i, s) in t.steps.iter().enumerate() {
        uses_of.insert(i, s.uses.iter().map(|u| bound.get(u).copied().ok_or_else(|| format!("tasks: unbound {u}"))).collect::<LangResult<Vec<u32>>>()?);
        if let Some(b) = &s.bind {
            bound.insert(b.clone(), g.steps[i].value);
        }
    }
    let mut pr = Printer { graphs, records: &p.records, out: String::new(), calls: BTreeSet::new() };
    let mut body = String::new();
    let mut all = vec![];
    for (k, id) in params.iter().enumerate() {
        body.push_str(&format!("  const p{id} = Promise.resolve(args[{k}]);\n"));
    }
    for n in &g.body.nodes {
        if matches!(n.op, Op::Param { .. }) {
            continue;
        }
        let mut ext = BTreeSet::new();
        externals(n, &mut ext);
        // The accept query takes the step's used names besides its inputs.
        if let (Op::Effect { .. }, Some(i)) = (&n.op, main.get(&n.id)) {
            ext.extend(uses_of[i].iter().copied());
        }
        let deps: Vec<u32> = ext.into_iter().collect();
        let names = deps.iter().map(|d| format!("v{d}")).collect::<Vec<_>>().join(", ");
        let promises = deps.iter().map(|d| format!("p{d}")).collect::<Vec<_>>().join(", ");
        body.push_str(&format!("  const p{} = (async () => {{\n", n.id));
        body.push_str(&format!("    let [{names}] = [];\n    try {{ [{names}] = await Promise.all([{promises}]); }} catch {{ throw __skip; }}\n"));
        let step = main.get(&n.id).or_else(|| tries.get(&n.id)).copied();
        if let Some(i) = step {
            body.push_str(&format!("    if ({i} > ctl.failedAt) throw __skip;\n"));
        }
        match (&n.op, step) {
            (Op::Effect { name: effect, .. }, Some(i)) => {
                let s = &t.steps[i];
                let values = n.inputs.iter().map(|x| format!("v{x}")).collect::<Vec<_>>().join(", ");
                let uses = uses_of[&i].iter().map(|x| format!("v{x}")).collect::<Vec<_>>().join(", ");
                let accept = serde_json::to_string(s.accept.as_ref().ok_or("tasks: accept")?).unwrap();
                let quoted = serde_json::to_string(effect).unwrap();
                body.push_str(&format!(
                    "    return run(async () => {{\n      const values = [{values}];\n      const record = {{step: {i}, effect: {quoted}, args: values, outcome: 'accepted'}};\n      let reply, failed = false;\n      if (replay) {{ const r = replay.get({i}); if (!r || r.effect !== {quoted}) throw Error('trace has no reply for step {}'); if (Object.hasOwn(r, 'error')) {{ failed = true; record.error = r.error; }} else {{ reply = r.reply; record.reply = reply; }} }}\n      else {{ try {{ const h = effects[{quoted}]; if (typeof h !== 'function') throw Error('no handler for effect ' + {quoted}); if ({i} > ctl.failedAt) throw Error('an earlier step ended the task'); const own = new AbortController(); controllers.set({i}, own); try {{ reply = await h(...values, {{effect: declared[{quoted}], step: {i}, task: {task_name}, signal: signal ? AbortSignal.any([own.signal, signal]) : own.signal}}); }} finally {{ controllers.delete({i}); }} record.reply = reply; }} catch (e) {{ failed = true; record.error = String(e?.message ?? e); }} }}\n      let accepted = false;\n      if (!failed) {{ try {{ accepted = (await call({accept}, [{uses}{comma}reply])) === true; }} catch {{ accepted = false; }} }}\n      ctl.records.push(record);\n      if (accepted) return reply;\n",
                    i + 1,
                    task_name = serde_json::to_string(&t.name).unwrap(),
                    comma = if uses.is_empty() { "" } else { ", " },
                ));
                if let (Some(_), Some(region)) = (&s.fallback, n.regions.get(1)) {
                    pr.out.clear();
                    pr.region(&g.body, region, 6)?;
                    body.push_str("      record.outcome = 'fallback';\n");
                    body.push_str(&pr.out);
                    body.push_str(&format!("      return v{};\n", region.result.ok_or("tasks: fallback result")?));
                }
                body.push_str(&format!(
                    "      record.outcome = 'failed';\n      throw new TaskFailed(failed ? 'effect ' + {quoted} + ' failed: ' + record.error : 'effect ' + {quoted} + ' returned a value its declared type or contract rejects');\n    }});\n"
                ));
            }
            (Op::Action { name: action, .. }, Some(_)) => {
                let values = n.inputs.iter().map(|x| format!("v{x}")).collect::<Vec<_>>().join(", ");
                body.push_str(&format!("    return run(() => call({}, [{values}]));\n", serde_json::to_string(action).unwrap()));
            }
            (Op::Try, Some(i)) => {
                body.push_str(&format!("    if (__isErr(v{0})) {{ fail({i}, {{result: v{0}}}); throw __skip; }}\n    return v{0}.Ok;\n", n.inputs[0]));
            }
            _ => {
                pr.out.clear();
                pr.node(&g.body, n, 4)?;
                body.push_str(&pr.out);
                body.push_str(&format!("    return v{};\n", n.id));
            }
        }
        // Failures register at their own step only; dependents skip.
        let index = step.map(|i| i.to_string()).unwrap_or_else(|| "-1".into());
        // A failing pure node is a host error that only an earlier step failure outranks.
        body.push_str(&format!("  }})().catch(e => {{ if (e !== __skip) fail({index} < 0 ? 1e9 : {index}, {{error: e}}); throw __skip; }});\n"));
        all.push(format!("p{}", n.id));
    }
    let result = g.body.result.ok_or("tasks: no result")?;
    let calls = pr.calls.clone();
    let mut helpers = String::new();
    let mut done = BTreeSet::new();
    let mut pending: Vec<String> = calls.into_iter().collect();
    while let Some(f) = pending.pop() {
        if !done.insert(f.clone()) {
            continue;
        }
        let graph = graphs.iter().find(|x| x.name == f && x.kind == Kind::Function).ok_or_else(|| format!("tasks: {f} is not a printable function"))?;
        let mut sub = Printer { graphs, records: &p.records, out: String::new(), calls: BTreeSet::new() };
        sub.graph(graph)?;
        helpers.push_str(&sub.out);
        pending.extend(sub.calls);
    }
    let declared = serde_json::to_string(&p.contracts.tasks.effects.iter().map(|e| (e.name.clone(), serde_json::to_value(e).unwrap())).collect::<BTreeMap<_, _>>()).unwrap();
    Ok(format!(
        "{helpers}async function {fname}(state, args, effects = {{}}, options = {{}}) {{\n  if (!Array.isArray(args) || args.length !== {arity}) throw TypeError('task {tname} takes {arity} arguments');\n  const call = __caller(state), signal = options.signal, run = __limit(options.concurrency ?? 16);\n  const declared = {declared};\n  const replay = options.replay ? new Map(options.replay.effects.map(r => [r.step, r])) : null;\n  const ctl = {{failedAt: Infinity, failure: null, records: []}};\n  const controllers = new Map();\n  const fail = (i, f) => {{ if (i < ctl.failedAt) {{ ctl.failedAt = i; ctl.failure = f; for (const [j, c] of controllers) if (j > i) c.abort(Error('an earlier step ended the task')); }} }};\n{body}  await Promise.allSettled([{all}]);\n  for (const r of ctl.records) if (r.step > ctl.failedAt) r.outcome = 'discarded';\n  ctl.records.sort((a, b) => a.step - b.step);\n  const trace = {{format: 'ink-task-trace-v1', task: {tname_json}, args, effects: ctl.records}};\n  if (ctl.failure?.error) {{ const e = ctl.failure.error; if (!(e instanceof TaskFailed)) throw e; trace.error = e.message; e.trace = trace; throw e; }}\n  trace.result = ctl.failure ? ctl.failure.result : await p{result};\n  return {{result: trace.result, trace}};\n}}\n",
        fname = name(&format!("task_{}", t.name)),
        arity = t.params.len(),
        tname = t.name,
        tname_json = serde_json::to_string(&t.name).unwrap(),
        all = all.join(", "),
    ))
}

/// The compiled tasks of a module, appended to its JavaScript as
/// `export const tasks` and `TaskFailed`; `None` when it declares no tasks.
pub fn emit(module: &CheckedModule) -> LangResult<Option<String>> {
    let p = module.program();
    if p.contracts.tasks.tasks.is_empty() {
        return Ok(None);
    }
    let m = lower(module);
    let mut out = String::from("export const {tasks, TaskFailed} = (() => {\n");
    out.push_str(HELPERS);
    out.push_str(RUNTIME);
    let mut entries = vec![];
    for t in &p.contracts.tasks.tasks {
        let g = m.graphs.iter().find(|g| g.name == t.name && g.kind == Kind::Task).ok_or_else(|| format!("tasks: {} did not lower: {}", t.name, m.unsupported.get(&t.name).cloned().unwrap_or_default()))?;
        out.push_str(&task(g, module, &m.graphs)?);
        entries.push(format!("{}: {}", serde_json::to_string(&t.name).unwrap(), name(&format!("task_{}", t.name))));
    }
    out.push_str(&format!("return {{tasks: {{{}}}, TaskFailed}};\n}})();\n", entries.join(", ")));
    // Every step is accounted for: effects, actions and tries carry a manifest index.
    debug_assert!(p.contracts.tasks.tasks.iter().all(|t| t.steps.iter().all(|s| matches!(s.kind, StepKind::Effect | StepKind::Query | StepKind::Change | StepKind::Value))));
    Ok(Some(out))
}
