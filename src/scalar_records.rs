//! Private, bounded scalar calling conventions for constructed records.
//! No arithmetic identity or derivative rule is used here: identical total
//! scalar computations on immutable carriers may share one evaluated local.
use super::{Emitter, Env};
use ink_core::{analysis, recursion, syntax::*};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Default)]
pub(super) struct LocalValues {
    pub enabled: bool,
    candidates: BTreeSet<String>,
    memo: BTreeMap<String, String>,
    literals: BTreeMap<String, String>,
    records: BTreeMap<String, Vec<(String, String)>>,
    record_bytes: usize,
}

// Only straight-line scopes share computations. Loops, lazy arms, callback
// bodies and recursion retain ordinary emission; no cached local crosses a
// control-flow edge. Named calls are never commoned and preserve strict work.
pub(super) fn straight(f: &Function, p: &Program) -> bool {
    if recursion::recursive(f) {
        return false;
    }
    let mut pending = vec![&f.body];
    let mut work = 16_384;
    while let Some(e) = pending.pop() {
        if work == 0 {
            return false;
        }
        work -= 1;
        match e {
            Expr::Let(..)
            | Expr::Record(..)
            | Expr::Field(..)
            | Expr::Var(..)
            | Expr::Num(..)
            | Expr::Float(..)
            | Expr::Double(..)
            | Expr::Bool(..)
            | Expr::String(..)
            | Expr::Unit
            | Expr::Neg(..) => {}
            Expr::Binary(op, _, _) if !matches!(op.as_str(), "&&" | "||") => {}
            Expr::Call(n, _)
                if ink_core::primitives::primitive(n)
                    || n == "sqrt"
                    || p.functions.iter().any(|f| &f.name == n) => {}
            _ => return false,
        }
        pending.extend(analysis::children(e));
    }
    true
}

impl LocalValues {
    pub fn for_function(f: &Function, p: &Program) -> Self {
        Self {
            enabled: straight(f, p),
            ..Self::default()
        }
    }
    pub fn record(&mut self, value: &str, slots: Vec<(String, String)>) {
        let bytes = value.len() + slots.iter().map(|(n, v)| n.len() + v.len()).sum::<usize>();
        if slots.len() <= 64
            && self.records.len() < 4096
            && self.record_bytes + bytes <= 262_144
            && !self.records.contains_key(value)
        {
            self.record_bytes += bytes;
            self.records.insert(value.to_owned(), slots);
        }
    }
    pub fn slots(&self, value: &str) -> Option<Vec<(String, String)>> {
        self.records.get(value).cloned()
    }
    pub fn field(&self, value: &str, name: &str) -> Option<String> {
        self.records
            .get(value)?
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.clone())
    }
    pub fn reuse(&self, value: &str) -> Option<String> {
        if self.enabled && self.candidates.contains(value) {
            self.memo.get(value).cloned()
        } else {
            None
        }
    }
    pub fn bound(&mut self, value: &str, name: &str) {
        if self.enabled && self.candidates.contains(value) {
            self.memo.insert(value.to_owned(), name.to_owned());
        }
        if self.literals.len() < 16_384 {
            if let Some(literal) = self.literals.get(value).cloned() {
                self.literals.insert(name.to_owned(), literal);
            }
        }
        if let Some(record) = self.records.get(value).cloned() {
            self.record(name, record);
        }
    }
    pub fn expression(&mut self, e: &Expr, value: &str, captured: bool, ty: Type) {
        // Only fixed IEEE scalar computations qualify. Do not
        // common arbitrary primitives, calls, mutable loads or host getters.
        if !matches!(ty, Type::F32 | Type::F64) {
            return;
        }
        let literal = match e {
            Expr::Num(_) => true,
            Expr::Float(bits) => f32::from_bits(*bits).is_finite(),
            Expr::Double(bits) => f64::from_bits(*bits).is_finite(),
            _ => false,
        };
        if literal && self.literals.len() < 16_384 {
            self.literals.insert(value.to_owned(), value.to_owned());
        }
        if literal
            || matches!(e, Expr::Neg(_))
            || matches!(e, Expr::Binary(op,_,_) if matches!(op.as_str(), "+"|"-"|"*"))
            || captured && matches!(e, Expr::Field(..))
        {
            if self.candidates.len() < 16_384 {
                self.candidates.insert(value.to_owned());
            }
        }
    }
}

#[derive(Clone)]
enum Shape {
    Record(Vec<(String, Shape)>),
    Constant(String, Type),
    Slot(usize, Type, bool),
}
impl Shape {
    fn key(&self) -> String {
        match self {
            Self::Record(fs) => format!(
                "R({})",
                fs.iter()
                    .map(|(n, s)| format!("{}:{}", super::quote(n), s.key()))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            Self::Constant(v, t) => format!("C{t:?}:{v}"),
            Self::Slot(i, t, number) => format!("S{i}:{t:?}:{number}"),
        }
    }
    fn bind(&self, e: &mut Emitter<'_>, vars: &[String]) -> String {
        match self {
            Self::Record(fs) => {
                let fields = fs
                    .iter()
                    .map(|(n, s)| (n.clone(), s.bind(e, vars)))
                    .collect();
                let v = e.fresh();
                e.split.insert(v.clone(), fields);
                v
            }
            Self::Constant(value, ty) => {
                e.local_values.literals.insert(value.clone(), value.clone());
                e.local_values.candidates.insert(value.clone());
                if *ty == Type::F32 && e.native_f32 {
                    e.f32_numbers.insert(value.clone());
                }
                e.bind(value)
            }
            Self::Slot(i, ty, number) => {
                let v = vars[*i].clone();
                if *ty == Type::F32 && e.native_f32 {
                    e.normalized_f32.insert(v.clone());
                    if *number {
                        e.f32_numbers.insert(v.clone());
                    }
                }
                v
            }
        }
    }
    fn rebuild(&self, vars: &[String]) -> String {
        match self {
            Self::Record(fs) => format!(
                "({{{}}})",
                fs.iter()
                    .map(|(n, s)| format!("[{}]:{}", super::quote(n), s.rebuild(vars)))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            Self::Constant(v, _) => v.clone(),
            Self::Slot(i, _, _) => vars[*i].clone(),
        }
    }
}

#[derive(Clone)]
struct Outline {
    index: usize,
    shapes: Vec<Shape>,
    slots: usize,
    reserved: usize,
}
#[derive(Clone, Default)]
pub(super) struct Outlines {
    tasks: Vec<Outline>,
    keys: BTreeMap<String, usize>,
    work: usize,
    reserved: usize,
    emitted: usize,
    blocked: BTreeSet<usize>,
}
impl Outlines {
    pub fn new(p: &Program) -> Self {
        let blocked = analysis::recursive_function_groups(p)
            .map(|groups| groups.into_iter().flatten().collect())
            .unwrap_or_else(|_| (0..p.functions.len()).collect());
        Self {
            blocked,
            ..Self::default()
        }
    }
}

fn shape(
    e: &Emitter<'_>,
    value: &str,
    ty: &Type,
    args: &mut Vec<String>,
    work: &mut usize,
    depth: usize,
) -> Option<Shape> {
    if *work == 0 || depth > 8 {
        return None;
    }
    *work -= 1;
    if let Type::Named(n) = ty {
        if !e.equality.elisions.contains(n) {
            if let Some(fields) = e.p.records.get(n) {
                let known = e
                    .split
                    .get(value)
                    .or_else(|| e.local_values.records.get(value));
                if let Some(known) = known {
                    let fs = fields
                        .iter()
                        .map(|(n, t)| {
                            let v = &known.iter().find(|(f, _)| f == n)?.1;
                            Some((n.clone(), shape(e, v, t, args, work, depth + 1)?))
                        })
                        .collect::<Option<Vec<_>>>()?;
                    return Some(Shape::Record(fs));
                }
            }
        }
    }
    if let Some(literal) = e.local_values.literals.get(value) {
        return Some(Shape::Constant(literal.clone(), ty.clone()));
    }
    let slot = args.len();
    args.push(value.to_owned());
    Some(Shape::Slot(slot, ty.clone(), e.f32_numbers.contains(value)))
}

pub(super) fn call(
    e: &mut Emitter<'_>,
    index: usize,
    f: &Function,
    actuals: &[String],
) -> Option<String> {
    if e.outlines.blocked.contains(&index)
        || recursion::recursive(f)
        || f.params
            .iter()
            .any(|(_, t)| ink_core::local_collections::owned(t, e.p))
        || ink_core::local_collections::owned(&f.result, e.p)
    {
        return None;
    }
    let mut args = vec![];
    let mut work = 512;
    let shapes = f
        .params
        .iter()
        .zip(actuals)
        .map(|((_, t), v)| shape(e, v, t, &mut args, &mut work, 0))
        .collect::<Option<Vec<_>>>()?;
    if !shapes.iter().any(|s| matches!(s, Shape::Record(_))) {
        return None;
    }
    let key = format!(
        "{index}:{}",
        shapes.iter().map(Shape::key).collect::<Vec<_>>().join(";")
    );
    let id = if let Some(id) = e.outlines.keys.get(&key) {
        *id
    } else {
        let nodes = analysis::node_count(&f.body, 16_384usize.saturating_sub(e.outlines.work))?;
        if e.outlines.tasks.len() >= 32 {
            return None;
        }
        let id = e.outlines.tasks.len();
        // Reserve a complete ordinary reconstruction wrapper before admitting
        // a call. Optional body/source exhaustion can always use this wrapper.
        let placeholders = vec!["v18446744073709551615".to_owned(); args.len()];
        let reserved = format!(
            "function __record31({}){{\nreturn __fn{index}({});}}\n",
            placeholders.join(","),
            shapes
                .iter()
                .map(|s| s.rebuild(&placeholders))
                .collect::<Vec<_>>()
                .join(",")
        )
        .len();
        if e.outlines.emitted + e.outlines.reserved + reserved > 262_144 {
            return None;
        }
        e.outlines.reserved += reserved;
        e.outlines.tasks.push(Outline {
            index,
            shapes,
            slots: args.len(),
            reserved,
        });
        e.outlines.keys.insert(key, id);
        e.outlines.work += nodes;
        id
    };
    Some(format!("__record{id}({})", args.join(",")))
}

pub(super) fn emit(e: &mut Emitter<'_>, javascript: &mut String) {
    let mut index = 0;
    while index < e.outlines.tasks.len() {
        let task = e.outlines.tasks[index].clone();
        e.outlines.reserved -= task.reserved;
        let f = e.p.functions[task.index].clone();
        let vars = (0..task.slots).map(|_| e.fresh()).collect::<Vec<_>>();
        let fallback = format!(
            "return __fn{}({});",
            task.index,
            task.shapes
                .iter()
                .map(|s| s.rebuild(&vars))
                .collect::<Vec<_>>()
                .join(",")
        );
        e.lines.clear();
        e.local_values = LocalValues::for_function(&f, e.p);
        for v in &vars {
            e.immutable_locals.insert(v.clone());
        }
        let env: Env = f
            .params
            .iter()
            .zip(&task.shapes)
            .map(|((n, t), s)| (n.clone(), (s.bind(e, &vars), t.clone())))
            .collect();
        // Each outlined body starts a fresh target stack, with the original
        // admission bound. Its optional work cannot reject the original module.
        let budget = e.budget;
        e.budget = 16_384;
        e.in_place =
            ink_core::in_place::analyse_with_result(&f.body, &f.params, Some(&f.result), e.p);
        let outlines = e.outlines.clone();
        let split = e.split.clone();
        let immutable = e.immutable_locals.clone();
        let floats = e.f32_numbers.clone();
        let normalized = e.normalized_f32.clone();
        let bit_floats = e.bit_f32_numbers.clone();
        let next = e.next;
        let inline_nodes = e.inline_nodes;
        let inline_source = e.inline_source;
        let float_scan = e.float_scan_work;
        e.float_scan_work = 512;
        let callbacks = e.callbacks.clone();
        let equality = e.equality.clone();
        let result = e.expr(&f.body, &env, Some(&f.result));
        e.budget = budget;
        let header = format!("function __record{index}({}){{\n", vars.join(","));
        let body = match result {
            Ok(value)
                if e.lines.len() + value.len() <= 65_536
                    && e.outlines.emitted
                        + e.outlines.reserved
                        + header.len()
                        + e.lines.len()
                        + value.len()
                        + 10
                        <= 262_144 =>
            {
                format!("{}return {value};", e.lines)
            }
            _ => {
                e.outlines = outlines;
                e.callbacks = callbacks;
                e.equality = equality;
                e.split = split;
                e.immutable_locals = immutable;
                e.f32_numbers = floats;
                e.normalized_f32 = normalized;
                e.bit_f32_numbers = bit_floats;
                e.next = next;
                e.inline_nodes = inline_nodes;
                e.inline_source = inline_source;
                e.float_scan_work = float_scan;
                fallback
            }
        };
        let definition = format!("{header}{body}}}\n");
        e.outlines.emitted += definition.len();
        javascript.push_str(&definition);
        index += 1;
    }
}

// Only already-constructed private fields qualify. Borrowed host records never
// enter this table, so projection retains all accessor reads and exceptions.
pub(super) fn projection(e: &Emitter<'_>, expr: &Expr, env: &Env) -> Option<String> {
    match expr {
        Expr::Var(n) => Some(env.get(n)?.0.clone()),
        Expr::Field(a, n) => {
            let root = projection(e, a, env)?;
            let fields = e
                .split
                .get(&root)
                .or_else(|| e.local_values.records.get(&root))?;
            Some(fields.iter().find(|(f, _)| f == n)?.1.clone())
        }
        _ => None,
    }
}
