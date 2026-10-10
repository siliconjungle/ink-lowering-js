//! Literal JavaScript emission for checked pure Ink, with no optimisation laws.
use ink_core::{check, core::CheckedModule, syntax::*, LangResult};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub struct Emission {
    pub javascript: String,
    pub manifest: Value,
}
mod state;
type Env = BTreeMap<String, (String, Type)>;
fn quote(s: &str) -> String {
    serde_json::to_string(s).unwrap()
}
fn descriptor(t: &Type, p: &Program, seen: &mut BTreeSet<String>) -> LangResult<Value> {
    Ok(match t {
        Type::U32 => json!({"kind":"u32"}), Type::U64 => json!({"kind":"u64"}),
        Type::I32 => json!({"kind":"i32"}), Type::F32 => json!({"kind":"f32"}),
        Type::Bool => json!({"kind":"bool"}),
        Type::Unit => json!({"kind":"unit"}), Type::String => json!({"kind":"string"}), Type::Int => json!({"kind":"int"}),
        Type::Option(t) => json!({"kind":"option","element":descriptor(t,p,seen)?}),
        Type::Result(a,b) => json!({"kind":"result","ok":descriptor(a,p,seen)?,"error":descriptor(b,p,seen)?}),
        Type::List(t) => json!({"kind":"list","element":descriptor(t,p,seen)?}),
        Type::Vector(t,n) => json!({"kind":"vector","size":n,"element":descriptor(t,p,seen)?}),
        Type::Named(n) if p.ids.contains(n) => json!({"kind":"id","name":n}),
        Type::Named(n) if n == "ArithmeticError" => json!({"kind":"enum","name":n,"variants":["ArithmeticError.Overflow"]}),
        Type::Named(n) if p.enums.contains_key(n) => json!({"kind":"enum","name":n,"variants":p.enums[n].iter().map(|v|format!("{n}.{v}")).collect::<Vec<_>>()}),
        Type::Named(n) => {
            if !seen.insert(n.clone()) { return Ok(json!({"kind":"ref","name":n})); }
            let fs=p.records.get(n).ok_or("JavaScript lowering requires a record definition")?;
            let fields=fs.iter().map(|(name,t)|Ok(json!({"name":name,"type":descriptor(t,p,seen)?})))
                .collect::<LangResult<Vec<_>>>()?;
            seen.remove(n);
            json!({"kind":"record","name":n,"fields":fields})
        }
        _ => return Err(format!("JavaScript lowering does not support {t:?}; supports pure numeric, Bool, vector, record and list values")),
    })
}
fn arithmetic(op: &str, a: &str, b: &str, t: &Type) -> String {
    match t {
        Type::U64 => format!("__u64(({a}) {op} ({b}))"),
        Type::U32 => {
            if op == "*" {
                format!("(Math.imul({a},{b}) >>> 0)")
            } else {
                format!("((({a}) {op} ({b})) >>> 0)")
            }
        }
        Type::I32 => {
            if op == "*" {
                format!("Math.imul({a},{b})")
            } else {
                format!("((({a}) {op} ({b})) | 0)")
            }
        }
        Type::F32 => format!("__bits(__float({a}) {op} __float({b}))"),
        _ => unreachable!("checked arithmetic type"),
    }
}
fn zero(t: &Type) -> &'static str {
    if *t == Type::U64 {
        "0n"
    } else {
        "0"
    }
}
struct Emitter<'a> {
    p: &'a Program,
    next: usize,
    budget: usize,
}
impl Emitter<'_> {
    fn fresh(&mut self) -> String {
        let n = format!("v{}", self.next);
        self.next += 1;
        n
    }
    fn types(env: &Env) -> check::Env {
        env.iter()
            .map(|(n, (_, t))| (n.clone(), t.clone()))
            .collect()
    }
    fn infer(&self, e: &Expr, env: &Env, want: Option<&Type>) -> LangResult<Type> {
        check::infer_context(e, &Self::types(env), self.p, want)
    }
    fn lambda(
        &mut self,
        e: &Expr,
        env: &Env,
        params: &[Type],
        result: &Type,
    ) -> LangResult<String> {
        let mut local = env.clone();
        let mut vars = vec![];
        let mut body = e;
        for t in params {
            let Expr::Lambda(n, b) = body else {
                return Err("expected collection lambda".into());
            };
            let v = self.fresh();
            local.insert(n.clone(), (v.clone(), t.clone()));
            vars.push(v);
            body = b;
        }
        let body = self.expr(body, &local, Some(result))?;
        Ok(format!(
            "({})=>{{__tick(ctx);return {body};}}",
            vars.join(",")
        ))
    }
    fn expr(&mut self, e: &Expr, env: &Env, want: Option<&Type>) -> LangResult<String> {
        if self.budget == 0 {
            return Err("JavaScript emission budget exceeded".into());
        }
        self.budget -= 1;
        let t = self.infer(e, env, want)?;
        Ok(match e {
            Expr::Num(n) => match t {
                Type::U64 => format!("{n}n"),
                Type::F32 => format!("{}", (*n as f32).to_bits()),
                _ => n.to_string(),
            },
            Expr::Float(b) => b.to_string(),
            Expr::Bool(b) => b.to_string(),
            Expr::Var(n) => env.get(n).ok_or("unbound JavaScript variable")?.0.clone(),
            Expr::Neg(a) => {
                if t == Type::I32 {
                    if let Expr::Num(n) = a.as_ref() {
                        return Ok(format!("{}", (*n as i32).wrapping_neg()));
                    }
                }
                let a = self.expr(a, env, Some(&t))?;
                if t == Type::F32 {
                    format!("(({a} ^ 2147483648) >>> 0)")
                } else {
                    arithmetic("-", zero(&t), &a, &t)
                }
            }
            Expr::Let(n, annotation, a, b) => {
                let at = self.infer(a, env, annotation.as_deref())?;
                let av = self.expr(a, env, Some(&at))?;
                let var = self.fresh();
                let mut local = env.clone();
                local.insert(n.clone(), (var.clone(), at));
                let bv = self.expr(b, &local, Some(&t))?;
                format!("(()=>{{const {var}={av};return {bv};}})()")
            }
            Expr::Record(n, fields) => {
                let fs = fields
                    .iter()
                    .map(|(name, e)| {
                        let ft = &self.p.records[n].iter().find(|(f, _)| f == name).unwrap().1;
                        Ok(format!(
                            "[{}]:{}",
                            quote(name),
                            self.expr(e, env, Some(ft))?
                        ))
                    })
                    .collect::<LangResult<Vec<_>>>()?;
                format!("({{{}}})", fs.join(","))
            }
            Expr::Field(a, n) => {
                let at = self.infer(a, env, None)?;
                let av = self.expr(a, env, None)?;
                let field = if matches!(at, Type::Vector(..)) {
                    ["x", "y", "z", "w"]
                        .iter()
                        .position(|f| f == n)
                        .unwrap()
                        .to_string()
                } else {
                    quote(n)
                };
                format!("({av})[{field}]")
            }
            Expr::Binary(op, a, b) => {
                let ts = Self::types(env);
                let context = if check::is_number(&t) {
                    Some(t.clone())
                } else {
                    check::hint(a, &ts, self.p).or_else(|| check::hint(b, &ts, self.p))
                };
                let at = self.infer(a, env, context.as_ref())?;
                let av = self.expr(a, env, Some(&at))?;
                let bv = self.expr(b, env, Some(&at))?;
                if ["+", "-", "*"].contains(&op.as_str()) {
                    arithmetic(op, &av, &bv, &at)
                } else {
                    let (av, bv) = if at == Type::F32 {
                        (format!("__float({av})"), format!("__float({bv})"))
                    } else {
                        (av, bv)
                    };
                    let op = match op.as_str() {
                        "==" => "===",
                        "!=" => "!==",
                        _ => op,
                    };
                    format!("(({av}) {op} ({bv}))")
                }
            }
            Expr::Call(n, args) if ["vec2", "vec3", "vec4"].contains(&n.as_str()) => {
                let Type::Vector(inner, _) = &t else {
                    unreachable!()
                };
                format!(
                    "[{}]",
                    args.iter()
                        .map(|e| self.expr(e, env, Some(inner)))
                        .collect::<LangResult<Vec<_>>>()?
                        .join(",")
                )
            }
            Expr::Call(n, args) if n == "choose" => format!(
                "({}?{}:{})",
                self.expr(&args[0], env, Some(&Type::Bool))?,
                self.expr(&args[1], env, Some(&t))?,
                self.expr(&args[2], env, Some(&t))?
            ),
            Expr::Call(n, args) if n == "quot_or" || n == "rem_or" => format!(
                "__div({},{},()=>{}, {}, {})",
                self.expr(&args[0], env, Some(&t))?,
                self.expr(&args[1], env, Some(&t))?,
                self.expr(&args[2], env, Some(&t))?,
                t == Type::I32,
                n == "rem_or"
            ),
            Expr::Call(n, args) if n == "repeat" => {
                let Expr::Num(bound) = args[0] else {
                    unreachable!()
                };
                format!(
                    "__repeat({bound},{},{})",
                    self.expr(&args[1], env, Some(&t))?,
                    self.lambda(&args[2], env, &[Type::U32, t.clone()], &t)?
                )
            }
            Expr::Call(n, args) if n == "foldr" => {
                let Type::List(inner) = self.infer(&args[0], env, None)? else {
                    unreachable!()
                };
                format!(
                    "__foldr({},{},{})",
                    self.expr(&args[0], env, None)?,
                    self.expr(&args[1], env, Some(&t))?,
                    self.lambda(&args[2], env, &[*inner, t.clone()], &t)?
                )
            }
            Expr::Call(n, args) if n == "sum" || n == "count" => {
                let context = if n == "sum" {
                    Some(Type::List(Box::new(t.clone())))
                } else {
                    None
                };
                let xs = self.expr(&args[0], env, context.as_ref())?;
                if n == "count" {
                    format!("BigInt(({xs}).length)")
                } else {
                    format!(
                        "({xs}).reduce((a,b)=>{{__tick(ctx);return {}; }},{})",
                        arithmetic("+", "a", "b", &t),
                        zero(&t)
                    )
                }
            }
            Expr::Call(n, args) => {
                let (index, f) = self
                    .p
                    .functions
                    .iter()
                    .enumerate()
                    .find(|(_, f)| &f.name == n)
                    .ok_or("unknown JavaScript function")?;
                let values = args
                    .iter()
                    .zip(&f.params)
                    .map(|(e, (_, t))| self.expr(e, env, Some(t)))
                    .collect::<LangResult<Vec<_>>>()?;
                format!(
                    "__fn{index}(ctx{})",
                    if values.is_empty() {
                        String::new()
                    } else {
                        format!(",{}", values.join(","))
                    }
                )
            }
            Expr::Method(xs, n, args) => {
                let Type::List(inner) =
                    self.infer(xs, env, if n == "filter" { Some(&t) } else { None })?
                else {
                    unreachable!()
                };
                let xv = self.expr(xs, env, None)?;
                match n.as_str() {
                    "at_or" => format!(
                        "__at({xv},{},()=>{})",
                        self.expr(&args[0], env, Some(&Type::U32))?,
                        self.expr(&args[1], env, Some(&t))?
                    ),
                    "sort" => format!("__sort({xv})"),
                    "scan" => format!(
                        "__scan({xv},{},(a,b)=>{{__tick(ctx);return {};}})",
                        zero(&inner),
                        arithmetic("+", "a", "b", &inner)
                    ),
                    "map" | "filter" => {
                        let output = if n == "filter" {
                            Type::Bool
                        } else {
                            let Type::List(out) = &t else { unreachable!() };
                            *out.clone()
                        };
                        format!(
                            "({xv}).{n}({})",
                            self.lambda(&args[0], env, &[*inner], &output)?
                        )
                    }
                    "map_indexed" => {
                        let Type::List(out) = &t else { unreachable!() };
                        let f = self.lambda(&args[0], env, &[Type::U32, *inner], out)?;
                        // JavaScript map's callback order is element, index.
                        let v = self.fresh();
                        let i = self.fresh();
                        format!("({xv}).map(({v},{i})=>({f})({i},{v}))")
                    }
                    "zip" => {
                        let Type::List(other) = self.infer(&args[0], env, None)? else {
                            unreachable!()
                        };
                        let Type::List(out) = &t else { unreachable!() };
                        format!(
                            "__zip({xv},{},{})",
                            self.expr(&args[0], env, None)?,
                            self.lambda(&args[1], env, &[*inner, *other], out)?
                        )
                    }
                    _ => return Err(format!("unsupported JavaScript method {n}")),
                }
            }
            _ => return Err(
                "JavaScript lowering supports pure function expressions, not stateful operations"
                    .into(),
            ),
        })
    }
}
/// Emit an ES module. Backend implementation and the JavaScript engine are
/// trusted; checked source/rewrite evidence does not certify emitted machine code.
pub fn lower(module: &CheckedModule) -> LangResult<Emission> {
    lower_with_layout(module, None)
}
/// Emit selected code with the snapshot origin authenticated by the core.
pub fn lower_selected(selected: &ink_core::optimisation::CheckedSelection) -> LangResult<Emission> {
    let layout = ink_core::snapshot::selected_layout(selected)?;
    let mut emission = lower_with_layout(selected.module(), Some(&layout))?;
    emission.manifest["selection"] =
        serde_json::to_value(selected.evidence()).map_err(|e| e.to_string())?;
    Ok(emission)
}
fn lower_with_layout(
    module: &CheckedModule,
    layout: Option<&ink_core::snapshot_wire::Layout>,
) -> LangResult<Emission> {
    let p = module.program();
    let mut emitter = Emitter {
        p,
        next: 0,
        budget: 200_000,
    };
    let mut javascript = include_str!("values.mjs").to_string();
    javascript.push_str(
        "\nfunction __tick(ctx){if(--ctx.fuel<0)throw Error('Ink execution budget exhausted');}\n",
    );
    let mut functions = vec![];
    for (i, f) in p.functions.iter().enumerate() {
        let params = f
            .params
            .iter()
            .map(|(n, t)| Ok(json!({"name":n,"type":descriptor(t,p,&mut BTreeSet::new())?})))
            .collect::<LangResult<Vec<_>>>()?;
        let result = descriptor(&f.result, p, &mut BTreeSet::new())?;
        let mut env = Env::new();
        let mut vars = vec![];
        for (n, t) in &f.params {
            let v = emitter.fresh();
            env.insert(n.clone(), (v.clone(), t.clone()));
            vars.push(v);
        }
        let body = emitter.expr(&f.body, &env, Some(&f.result))?;
        javascript.push_str(&format!(
            "function __fn{i}(ctx{}){{__tick(ctx);return {body};}}\n",
            if vars.is_empty() {
                String::new()
            } else {
                format!(",{}", vars.join(","))
            }
        ));
        functions.push(json!({"index":i,"name":f.name,"params":params,"result":result}));
    }
    let named = p
        .records
        .keys()
        .map(|n| {
            Ok((
                n.clone(),
                descriptor(&Type::Named(n.clone()), p, &mut BTreeSet::new())?,
            ))
        })
        .collect::<LangResult<BTreeMap<_, _>>>()?;
    javascript.push_str(&format!(
        "const __namedTypes={};\n",
        serde_json::to_string(&named).map_err(|e| e.to_string())?
    ));
    let state = if !p.actions.is_empty() || !p.states.is_empty() || !p.keeps.is_empty() {
        let (code, metadata) = if let Some(layout) = layout {
            state::emit_with_layout(module, layout)?
        } else {
            state::emit(module)?
        };
        javascript.push_str(include_str!("snapshot.mjs"));
        javascript.push_str(&code);
        Some(metadata)
    } else {
        None
    };
    let interface = json!({"schema":1,"backend":"javascript","abi":"ink-javascript-values-v1","core_semantics":module.semantics(),"core_sha256":module.identity()?,"functions":functions,"state":state,"named_types":named});
    javascript.push_str(&format!(
        "export const metadata = {};\nconst __implementations=[{}];\n",
        interface,
        (0..p.functions.len())
            .map(|i| format!("__fn{i}"))
            .collect::<Vec<_>>()
            .join(",")
    ));
    javascript.push_str(r#"
function __freeze(v) { if(v && typeof v==='object') {Object.values(v).forEach(__freeze);Object.freeze(v);} return v; }
__freeze(metadata);
const __interfaces = new Map(metadata.functions.map(f=>[f.name,f]));
export function call(name, args) {
  const f=__interfaces.get(name);
  if (!f) throw Error(`unknown Ink function ${name}`);
  if (!Array.isArray(args) || args.length!==f.params.length) __fail('argument count mismatch');
  const budget={cells:1000000};
  const captured=args.map((v,i)=>__in(v,f.params[i].type,budget));
  return __out(__implementations[f.index]({fuel:1000000},...captured),f.result,{cells:1000000});
}
export const functions = Object.freeze(Object.fromEntries(metadata.functions.map(f=>[f.name,(...args)=>call(f.name,args)])));
"#);
    let mut manifest = interface;
    manifest["javascript_sha256"] = json!(format!("{:x}", Sha256::digest(javascript.as_bytes())));
    manifest["trusted"] = json!([
        "Ink checker and source correspondence",
        "JavaScript emitter and value codec",
        "JavaScript engine and host"
    ]);
    manifest["scope"] = json!(
        "literal pure and stateful execution; portable snapshots and ordered transaction events"
    );
    Ok(Emission {
        javascript,
        manifest,
    })
}
