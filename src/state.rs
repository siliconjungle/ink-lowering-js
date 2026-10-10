//! Literal state/action emission. The support runtime contains fixed primitives,
//! never program syntax or an evaluator.
use super::{arithmetic, descriptor, quote, zero, Env};
use ink_core::{syntax::*, LangResult};
use serde_json::{json, Value};
use std::collections::BTreeSet;
struct StateEmitter<'a> {
    p: &'a Program,
    next: usize,
    execution: &'a ink_core::action_ir::Execution,
}
impl StateEmitter<'_> {
    fn fresh(&mut self) -> String {
        let n = format!("s{}", self.next);
        self.next += 1;
        n
    }
    fn infer(&self, e: &Expr, _env: &Env, _want: Option<&Type>) -> LangResult<Type> {
        self.execution
            .type_of(e)
            .cloned()
            .or_else(|| {
                if let Expr::Var(n) = e {
                    self.p
                        .states
                        .iter()
                        .chain(&self.p.keeps)
                        .find(|d| &d.name == n)
                        .map(|d| d.ty.clone())
                } else {
                    None
                }
            })
            .ok_or_else(|| format!("missing checked action type: {e:?}"))
    }
    fn expr(&mut self, e: &Expr, env: &Env, want: Option<&Type>) -> LangResult<String> {
        let t = self.infer(e, env, want)?;
        Ok(match e {
            Expr::Num(n) => {
                if matches!(t, Type::U64 | Type::Int) {
                    format!("{n}n")
                } else {
                    n.to_string()
                }
            }
            Expr::Bool(b) => b.to_string(),
            Expr::String(s) => quote(s),
            Expr::Unit => "null".into(),
            Expr::Var(n) => {
                if let Some((v, _)) = env.get(n) {
                    v.clone()
                } else if n == "None" {
                    "({None:null})".into()
                } else {
                    let i = self
                        .p
                        .keeps
                        .iter()
                        .position(|k| &k.name == n)
                        .ok_or("table cannot be used as a value")?;
                    format!("__keep{i}(ctx,rt)")
                }
            }
            Expr::Field(x, n) => {
                if let Expr::Var(en) = x.as_ref() {
                    if self.p.enums.contains_key(en) {
                        return Ok(quote(&format!("{en}.{n}")));
                    }
                }
                format!("({})[{}]", self.expr(x, env, None)?, quote(n))
            }
            Expr::Record(n, fs) => {
                let mut fields = vec![];
                for (k, e) in fs {
                    let ft = &self.p.records[n]
                        .iter()
                        .find(|(f, _)| f == k)
                        .ok_or("record field")?
                        .1;
                    fields.push(format!("[{}]:{}", quote(k), self.expr(e, env, Some(ft))?))
                }
                format!("({{{}}})", fields.join(","))
            }
            Expr::Try(x) => format!("__try({})", self.expr(x, env, None)?),
            Expr::Binary(op, a, b) => {
                let mut at = self.infer(
                    a,
                    env,
                    if matches!(t, Type::U32 | Type::U64 | Type::Int) {
                        Some(&t)
                    } else {
                        None
                    },
                )?;
                if matches!(a.as_ref(), Expr::Num(_)) {
                    let bt = self.infer(b, env, None)?;
                    if matches!(bt, Type::U32 | Type::Int) {
                        at = bt
                    }
                }
                let a = self.expr(a, env, Some(&at))?;
                let b = self.expr(b, env, Some(&at))?;
                if ["+", "-", "*"].contains(&op.as_str()) {
                    if at == Type::Int {
                        format!("(({a}) {op} ({b}))")
                    } else {
                        arithmetic(op, &a, &b, &at)
                    }
                } else if op == "==" || op == "!=" {
                    format!("{}__equal({a},{b})", if op == "!=" { "!" } else { "" })
                } else {
                    format!("(({a}) {op} ({b}))")
                }
            }
            Expr::Call(n, args) => match n.as_str() {
                "Ok" | "Err" | "Some" => {
                    let inner = match (&t, n.as_str()) {
                        (Type::Result(a, _), "Ok") | (Type::Option(a), "Some") => Some(&**a),
                        (Type::Result(_, b), "Err") => Some(&**b),
                        _ => None,
                    };
                    format!("({{{n}:{}}})", self.expr(&args[0], env, inner)?)
                }
                "Int" => format!("BigInt({})", self.expr(&args[0], env, None)?),
                "checked_add" | "checked_sub" | "checked_mul" => {
                    let at = self.infer(&args[0], env, None)?;
                    format!(
                        "__checked({}, {}, {}, {})",
                        quote(n),
                        self.expr(&args[0], env, Some(&at))?,
                        self.expr(&args[1], env, Some(&at))?,
                        if at == Type::U32 { 32 } else { 64 }
                    )
                }
                "sum" | "count" => {
                    let xs = self.expr(&args[0], env, None)?;
                    if n == "count" {
                        format!("BigInt(({xs}).length)")
                    } else {
                        let add = if t == Type::Int {
                            "a+b".into()
                        } else {
                            arithmetic("+", "a", "b", &t)
                        };
                        format!(
                            "({xs}).reduce((a,b)=>{{__tick(ctx);return {add};}},{})",
                            if t == Type::Int { "0n" } else { zero(&t) }
                        )
                    }
                }
                _ => {
                    let (index, params, change, isaction) = if let Some((i, a)) = self
                        .p
                        .actions
                        .iter()
                        .enumerate()
                        .find(|(_, a)| &a.name == n)
                    {
                        (i, a.params.clone(), a.kind == ActionKind::Change, true)
                    } else {
                        let (i, f) = self
                            .p
                            .functions
                            .iter()
                            .enumerate()
                            .find(|(_, f)| &f.name == n)
                            .ok_or("call target")?;
                        (i, f.params.clone(), false, false)
                    };
                    let values = args
                        .iter()
                        .zip(&params)
                        .map(|(e, (_, t))| self.expr(e, env, Some(t)))
                        .collect::<LangResult<Vec<_>>>()?;
                    let argv = if values.is_empty() {
                        String::new()
                    } else {
                        format!(",{}", values.join(","))
                    };
                    if isaction {
                        format!("__nested(__action{index}(ctx,rt{argv}),{change})")
                    } else {
                        format!("__fn{index}(ctx{argv})")
                    }
                }
            },
            Expr::Method(xs, n, args) => {
                let xt = self.infer(xs, env, None)?;
                if let Type::Table(kt, vt) = xt {
                    let Expr::Var(root) = xs.as_ref() else {
                        return Err("table root".into());
                    };
                    let i = self
                        .p
                        .states
                        .iter()
                        .position(|d| &d.name == root)
                        .ok_or("table index")?;
                    let values = args
                        .iter()
                        .enumerate()
                        .map(|(i, e)| self.expr(e, env, Some(if i == 0 { &kt } else { &vt })))
                        .collect::<LangResult<Vec<_>>>()?;
                    format!("__table(rt,{i},{},[{}])", quote(n), values.join(","))
                } else {
                    let x = self.expr(xs, env, None)?;
                    if n == "ok_or" {
                        format!("__okOr({x},{})", self.expr(&args[0], env, None)?)
                    } else {
                        let inner = match &xt {
                            Type::List(t) | Type::Option(t) => t.clone(),
                            Type::Result(a, b) => {
                                if n == "map_err" {
                                    b.clone()
                                } else {
                                    a.clone()
                                }
                            }
                            _ => return Err("map receiver".into()),
                        };
                        let Expr::Lambda(v, body) = &args[0] else {
                            return Err("lambda".into());
                        };
                        let var = self.fresh();
                        let mut local = env.clone();
                        local.insert(v.clone(), (var.clone(), *inner));
                        let body = self.expr(body, &local, None)?;
                        let kind = match xt {
                            Type::List(_) => "list",
                            Type::Option(_) => "option",
                            _ => "result",
                        };
                        format!(
                            "__mapValue({x},{},{},({var})=>{{__tick(ctx);return {body};}})",
                            quote(kind),
                            quote(n)
                        )
                    }
                }
            }
            _ => return Err("unsupported checked action expression".into()),
        })
    }
    fn block(&mut self, body: &[Statement], env: &mut Env, result: &Type) -> LangResult<String> {
        let mut code = String::new();
        for s in body {
            code.push_str("__tick(ctx);");
            match s {
                Statement::Let(n, t, e) => {
                    let at = self.infer(e, env, t.as_ref())?;
                    let e = self.expr(e, env, Some(&at))?;
                    let v = self.fresh();
                    code.push_str(&format!("const {v}={e};"));
                    env.insert(n.clone(), (v, at));
                }
                Statement::Return(e) => {
                    code.push_str(&format!("return {};", self.expr(e, env, Some(result))?))
                }
                Statement::Expr(e) => code.push_str(&format!("{};", self.expr(e, env, None)?)),
                Statement::Emit(n, e) => code.push_str(&format!(
                    "rt.staged.push([{},{}]);",
                    quote(n),
                    self.expr(e, env, Some(&self.p.events[n]))?
                )),
                Statement::If(e, a, b) => {
                    let e = self.expr(e, env, Some(&Type::Bool))?;
                    let a = self.block(a, &mut env.clone(), result)?;
                    let b = self.block(b, &mut env.clone(), result)?;
                    code.push_str(&format!("if({e}){{{a}}}else{{{b}}}"));
                }
            }
        }
        Ok(code)
    }
}
pub fn emit(module: &ink_core::core::CheckedModule) -> LangResult<(String, Value)> {
    let p = module.program();
    let execution = ink_core::action_ir::CheckedActions::elaborate(module)?.execution()?;
    let mut emitter = StateEmitter {
        p,
        next: 0,
        execution: &execution,
    };
    let mut code = include_str!("state.mjs").to_string();
    let mut actions = vec![];
    for (i, k) in execution.keeps().iter().enumerate() {
        let e = emitter.expr(&k.value, &Env::new(), Some(&k.ty))?;
        code.push_str(&format!(
            "function __keep{i}(ctx,rt){{__tick(ctx);{}return {e};{}}}\n",
            if matches!(k.ty, Type::Result(..)) {
                "try{"
            } else {
                ""
            },
            if matches!(k.ty, Type::Result(..)) {
                "}catch(e){if(e instanceof __Abort)return {Err:e.value};throw e;}"
            } else {
                ""
            }
        ));
    }
    for (i, a) in execution.actions().iter().enumerate() {
        let mut env = Env::new();
        let mut vars = vec![];
        for (n, t) in &a.params {
            let v = emitter.fresh();
            env.insert(n.clone(), (v.clone(), t.clone()));
            vars.push(v)
        }
        let body = emitter.block(&a.body, &mut env, &a.result)?;
        code.push_str(&format!("function __action{i}(ctx,rt{}){{__tick(ctx);try{{{body}}}catch(e){{if(e instanceof __Abort)return {{Err:e.value}};throw e;}}}}\n",if vars.is_empty(){String::new()}else{format!(",{}",vars.join(","))}));
        actions.push(json!({"index":i,"name":a.name,"change":a.kind==ActionKind::Change,"params":a.params.iter().map(|(n,t)|Ok(json!({"name":n,"type":descriptor(t,p,&mut BTreeSet::new())?}))).collect::<LangResult<Vec<_>>>()?,"result":descriptor(&a.result,p,&mut BTreeSet::new())?}));
    }
    let roots=p.states.iter().map(|s|{let Type::Table(k,v)=&s.ty else{return Err("state root".into())};Ok(json!({"name":s.name,"key":descriptor(k,p,&mut BTreeSet::new())?,"value":descriptor(v,p,&mut BTreeSet::new())?}))}).collect::<LangResult<Vec<_>>>()?;
    let events = p
        .events
        .iter()
        .map(|(n, t)| Ok(json!({"name":n,"type":descriptor(t,p,&mut BTreeSet::new())?})))
        .collect::<LangResult<Vec<_>>>()?;
    let layout = ink_core::snapshot::layout(p)?;
    let state = json!({"actions":actions,"roots":roots,"events":events,"snapshot":{"program":layout.program,"schema":layout.schema}});
    code.push_str(&format!(
        "const __stateMetadata={state};const __actions=[{}];\n",
        (0..p.actions.len())
            .map(|i| format!("__action{i}"))
            .collect::<Vec<_>>()
            .join(",")
    ));
    code.push_str(
        "export function createState(){return __createState(__stateMetadata,__actions);}\n",
    );
    Ok((code, state))
}
