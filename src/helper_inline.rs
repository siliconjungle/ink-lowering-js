//! Bounded literal helper expansion. This changes local storage and call
//! boundaries, not source arithmetic, contracts, or implementation selection.
use super::{Emitter, Env};
use ink_core::{analysis, local_collections, recursion, syntax::*};

pub(super) fn eligible(e: &Emitter<'_>, f: &Function) -> bool {
    if e.inline_depth >= 4
        || e.inline_nodes == 0
        || recursion::recursive(f)
        || local_collections::owned(&f.result, e.p)
        || f.params
            .iter()
            .any(|(_, t)| local_collections::owned(t, e.p))
        || !f
            .params
            .iter()
            .any(|(_, t)| matches!(t,Type::Named(n) if e.p.records.contains_key(n)))
    {
        return false;
    }
    // Leave stack headroom for the core's type/loan visitors as well as target
    // emission. The ordinary depth limit alone cannot protect analysis invoked
    // while an expanded caller's emission frames are still live.
    let mut pending = vec![(&f.body, e.depth)];
    let mut work = e.inline_nodes.min(512);
    while let Some((body, depth)) = pending.pop() {
        if depth >= 28 || work == 0 {
            return false;
        }
        work -= 1;
        pending.extend(
            analysis::children(body)
                .into_iter()
                .map(|child| (child, depth + 1)),
        );
    }
    true
}

pub(super) fn call(e: &mut Emitter<'_>, f: &Function, actuals: &[String]) -> Option<String> {
    if !actuals.iter().any(|v| e.split.contains_key(v)) {
        return None;
    }
    let nodes = analysis::node_count(&f.body, e.inline_nodes.min(512))?;
    e.inline_nodes -= nodes;
    let start = e.lines.len();
    let callbacks = e.callbacks.len();
    let equality = e.equality.clone();
    let split = e.split.clone();
    let immutable = e.immutable_locals.clone();
    let floats = e.f32_numbers.clone();
    let next = e.next;
    let budget = e.budget;
    let in_place = std::mem::replace(
        &mut e.in_place,
        ink_core::in_place::analyse_with_result(&f.body, &f.params, Some(&f.result), e.p),
    );
    let local: Env = f
        .params
        .iter()
        .zip(actuals)
        .map(|((n, t), v)| (n.clone(), (v.clone(), t.clone())))
        .collect();
    e.inline_depth += 1;
    let result = e.expr(&f.body, &local, Some(&f.result));
    e.inline_depth -= 1;
    e.in_place = in_place;
    // Optional expansion has its own work budget; it cannot consume the
    // ordinary source's admission budget or reject a supported original call.
    e.budget = budget;
    let bytes = e.lines.len() - start + result.as_ref().map_or(0, |value| value.len());
    if let Ok(value) = result {
        if bytes <= 65_536 && bytes <= e.inline_source {
            e.inline_source -= bytes;
            return Some(value);
        }
    }
    e.lines.truncate(start);
    e.callbacks.truncate(callbacks);
    e.equality = equality;
    e.split = split;
    e.immutable_locals = immutable;
    e.f32_numbers = floats;
    e.next = next;
    None
}
