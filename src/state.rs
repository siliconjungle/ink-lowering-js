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
    equality: super::TypedEquality<'a>,
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
                if t == Type::F32 {
                    return Ok((*n as f32).to_bits().to_string());
                }
                if matches!(t, Type::U64 | Type::Int) {
                    format!("{n}n")
                } else {
                    n.to_string()
                }
            }
            Expr::Bool(b) => b.to_string(),
            Expr::Float(bits) => bits.to_string(),
            Expr::Neg(value) => {
                if t == Type::I32 && matches!(value.as_ref(), Expr::Num(2147483648)) {
                    return Ok("-2147483648".into());
                }
                let v = self.expr(value, env, Some(&t))?;
                match t {
                    Type::F32 => format!("(({v})^0x80000000)>>>0"),
                    Type::I32 => format!("(-({v}))|0"),
                    Type::U32 => format!("(-({v}))>>>0"),
                    Type::U64 => format!("__u64(-({v}))"),
                    _ => format!("-({v})"),
                }
            }
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
                if matches!(self.infer(x, env, None)?, Type::Vector(..)) {
                    let v = self.expr(x, env, None)?;
                    let i = ["x", "y", "z", "w"]
                        .iter()
                        .position(|s| *s == n)
                        .ok_or("vector component")?;
                    return Ok(format!("({v})[{i}]"));
                }
                if matches!(self.infer(x,env,None)?,Type::Named(ref name) if self.equality.elisions.contains(name)) {return self.expr(x,env,None);}
                format!("({})[{}]", self.expr(x, env, None)?, quote(n))
            }
            Expr::Record(n,fs) if self.equality.elisions.contains(n) => self.expr(&fs[0].1,env,Some(&self.p.records[n][0].1))?,
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
                    if matches!(t, Type::U32 | Type::U64 | Type::Int | Type::I32 | Type::F32) {
                        Some(&t)
                    } else {
                        None
                    },
                )?;
                if matches!(a.as_ref(), Expr::Num(_)) {
                    let bt = self.infer(b, env, None)?;
                    if matches!(bt, Type::U32 | Type::Int | Type::I32 | Type::F32) {
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
                } else if at == Type::F32 {
                    let op = match op.as_str() {
                        "==" => "===",
                        "!=" => "!==",
                        _ => op,
                    };
                    format!("(__float({a}) {op} __float({b}))")
                } else if op == "==" || op == "!=" {
                    format!("{}({})", if op == "!=" { "!" } else { "" }, self.equality.expr(&at, &a, &b)?)
                } else {
                    format!("(({a}) {op} ({b}))")
                }
            }
            Expr::Let(n, annotation, value, body) => {
                let t = self.infer(value, env, annotation.as_deref())?;
                let value = self.expr(value, env, Some(&t))?;
                let variable = self.fresh();
                let mut local = env.clone();
                local.insert(n.clone(), (variable.clone(), t));
                format!(
                    "(({variable})=>({}))({value})",
                    self.expr(body, &local, want)?
                )
            }
            Expr::Call(n, args) => match n.as_str() {
                name if ink_core::primitives::primitive(name) => {
                    let first=args.first().and_then(|a|self.execution.type_of(a));
                    let (params,_)=ink_core::primitives::signature(name,first)?;
                    let vals=args.iter().zip(params).map(|(a,t)|self.expr(a,env,Some(&t))).collect::<LangResult<Vec<_>>>()?;
                    super::primitives::expression(name,&vals,first,false)?
                }

                name if ink_core::word::primitive(name) => {
                    let ts = ink_core::word::arguments(name, &t);
                    let vals = args
                        .iter()
                        .zip(ts)
                        .map(|(a, t)| self.expr(a, env, Some(&t)))
                        .collect::<LangResult<Vec<_>>>()?;
                    format!(
                        "__word({},{},{},{})",
                        quote(name),
                        vals[0],
                        vals.get(1).map(String::as_str).unwrap_or("0"),
                        match t {
                            Type::I32 => -32,
                            Type::U32 => 32,
                            _ => 64,
                        }
                    )
                }
                "choose" => format!(
                    "({}?{}:{})",
                    self.expr(&args[0], env, Some(&Type::Bool))?,
                    self.expr(&args[1], env, Some(&t))?,
                    self.expr(&args[2], env, Some(&t))?
                ),
                "list" => {
                    let Type::List(inner) = &t else {
                        return Err("list type".into());
                    };
                    format!(
                        "[{}]",
                        args.iter()
                            .map(|e| self.expr(e, env, Some(inner)))
                            .collect::<LangResult<Vec<_>>>()?
                            .join(",")
                    )
                }
                "vec2" | "vec3" | "vec4" => format!(
                    "[{}]",
                    args.iter()
                        .map(|e| self.expr(e, env, None))
                        .collect::<LangResult<Vec<_>>>()?
                        .join(",")
                ),
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
                    let Type::Result(at, _) = &t else {
                        return Err("checked arithmetic type".into());
                    };
                    let at = *at.clone();
                    format!(
                        "__checked({}, {}, {}, {})",
                        quote(n),
                        self.expr(&args[0], env, Some(&at))?,
                        self.expr(&args[1], env, Some(&at))?,
                        if at == Type::I32 {
                            -32
                        } else if at == Type::U32 {
                            32
                        } else {
                            64
                        }
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
                        (i, a.params.clone(), a.kind == ActionKind::Change && matches!(a.result,Type::Result(..)), true)
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
                        format!("__fn{index}({})", values.join(","))
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
                    if n=="sum_by" || n=="count_where" {
                        let Expr::Lambda(parameter,body)=&args[0] else {return Err("scan callback".into());};
                        let var=self.fresh();let mut local=env.clone();local.insert(parameter.clone(),(var.clone(),*vt));
                        let callback=self.expr(body,&local,None)?;
                        let add=if n=="count_where" {"acc+1n".into()} else if t==Type::Int {"acc+contribution".into()} else {arithmetic("+","acc","contribution",&t)};
                        let step=if n=="count_where" {format!("if(contribution)acc={add};")} else {format!("acc={add};")};
                        return Ok(format!("(()=>{{let acc={};for(const key of __orderedKeys(rt,{i})){{__tick(ctx);const {var}=__cellGet(rt,{i},key);const contribution={callback};{step}}}return acc;}})()",if t==Type::Int {"0n".into()}else{zero(&t)}));
                    }
                    if n=="shared_keys" {
                        let Expr::Var(other)=&args[0] else{return Err("shared_keys root".into());};
                        let j=self.p.states.iter().position(|s| &s.name==other).ok_or("shared_keys table")?;
                        return Ok(format!("__table(rt,{i},\"intersect_table\",[{j}])"));
                    }
                    let values = args
                        .iter()
                        .enumerate()
                        .map(|(i, e)| self.expr(e, env, Some(&ink_core::table_argument(n, i, &kt, &vt))))
                        .collect::<LangResult<Vec<_>>>()?;
                    // Fixed table primitives take typed arguments directly.
                    // Keep the generic dispatcher for compound operations only.
                    if n=="get"||n=="contains"||n=="remove" {
                        let helper=match n.as_str(){"get"=>"__tableGet","contains"=>"__tableContains",_=>"__tableRemove"};
                        return Ok(format!("{helper}(rt,{i},{})",values[0]));
                    }
                    if n=="put"||n=="insert"||n=="replace" {
                        let mode=if n=="insert"{1}else if n=="replace"{2}else{0};
                        return Ok(format!("__tablePut(rt,{i},{},{},{mode})",values[0],values[1]));
                    }
                    format!("__table(rt,{i},{},[{}])", quote(n), values.join(","))
                } else {
                    let x = self.expr(xs, env, None)?;
                    if n == "concat" {
                        format!("({x}).concat({})", self.expr(&args[0], env, Some(&xt))?)
                    } else if n == "fold" {
                        let (a, b, tag_a, tag_b) = match &xt {
                            Type::Option(a) => (a.as_ref().clone(), Type::Unit, "Some", "None"),
                            Type::Result(a, b) => {
                                (a.as_ref().clone(), b.as_ref().clone(), "Ok", "Err")
                            }
                            _ => return Err("fold receiver".into()),
                        };
                        let value = self.fresh();
                        let mut codes = vec![];
                        for (arg, input) in args.iter().zip([a, b]) {
                            let Expr::Lambda(v, body) = arg else {
                                return Err("fold lambda".into());
                            };
                            let variable = self.fresh();
                            let mut local = env.clone();
                            local.insert(v.clone(), (variable.clone(), input));
                            codes.push(format!(
                                "({variable})=>({})",
                                self.expr(body, &local, want)?
                            ));
                        }
                        format!("(({value})=>Object.hasOwn({value},{})?({})({value}[{}]):({})({}))({x})",quote(tag_a),codes[0],quote(tag_a),codes[1],if tag_b=="None"{"null".into()}else{format!("{value}[{}]",quote(tag_b))})
                    } else if n == "unwrap_or" {
                        format!("((v,fallback)=>Object.hasOwn(v,'Some')?v.Some:fallback)({x},{})", self.expr(&args[0], env, Some(&t))?)
                    } else if n == "ok_or" {
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
                    "__emit(rt,{},{});",
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
pub(crate) fn emit_with_elisions(
    module: &ink_core::core::CheckedModule,
    layout: &ink_core::snapshot_wire::Layout,
    elisions: &BTreeSet<String>,
) -> LangResult<(String, Value)> {
    let p = module.program();
    let checked_actions = ink_core::action_ir::CheckedActions::elaborate(module)?;
    let effects = ink_core::effects::CheckedEffects::derive(&checked_actions)?;
    let execution = checked_actions.execution()?;
    let mut emitter = StateEmitter {
        p,
        next: 0,
        execution: &execution,
        equality: super::TypedEquality::new(p, "__stateValueEqual"),
    };
    emitter.equality.elisions=elisions.clone();
    let mut code = include_str!("observers.mjs").to_string()+include_str!("state.mjs");
    let mut actions = vec![];
    let mut captures = super::state_capture::Captures::with_elisions(p,elisions);
    let mut capture_lists = Vec::new();
    let mut keeps = vec![];
    let mut equal_results=vec![];
    let equal_roots=p.states.iter().map(|s|{let Type::Table(_,t)=&s.ty else{return Err("state root".into())};Ok(format!("(a,b)=>{}",emitter.equality.expr(t,"a","b")?))}).collect::<LangResult<Vec<_>>>()?;
    let mut bound_factories=vec![];
    let mut keep_invalidations = vec![Vec::<usize>::new(); p.states.len()];
    for (i, k) in execution.keeps().iter().enumerate() {
        let e = emitter.expr(&k.value, &Env::new(), Some(&k.ty))?;
        // Retained values cannot carry mutable host aliases. Composite values
        // keep their existing ownership/observation path; scalar Int/F32 values
        // retain their internal exact carrier and convert at the public boundary.
        let cached = matches!(k.ty, Type::U32 | Type::U64 | Type::I32 | Type::F32 | Type::Int | Type::Bool | Type::String | Type::Unit)
            || matches!(&k.ty, Type::Named(n) if p.ids.contains(n) || p.enums.contains_key(n));
        let reads = &effects.keep(&k.name).ok_or("missing checked keep effects")?.access.reads;
        if cached {
            for (root, state) in p.states.iter().enumerate() {
                if reads.contains(&state.name) {
                    keep_invalidations[root].push(i);
                }
            }
        }
        keeps.push(json!({"name": k.name, "cached": cached}));
        code.push_str(&format!(
            "function {}{i}(ctx,rt){{__tick(ctx);{}return {e};{}}}\n",
            if cached { "__computeKeep" } else { "__keep" },
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
        if cached {
            code.push_str(&format!("function __keep{i}(ctx,rt){{return __cachedKeep(ctx,rt,{i},__computeKeep{i});}}\n"));
        }
    }
    for (i, a) in execution.actions().iter().enumerate() {
        capture_lists.push(format!("[{}]", a.params.iter().map(|(_, ty)| captures.for_type(ty)).collect::<LangResult<Vec<_>>>()?.join(",")));
        let params=(0..a.params.len()).map(|i|format!("p{i}")).collect::<Vec<_>>();
        let captures=(0..a.params.len()).map(|i|format!("inputs[{i}](p{i})")).collect::<Vec<_>>();
        // Bind a direct action entry once. Captures become lexical locals;
        // no argument carrier, spread invocation or name/kind dispatch remains.
        // Preconditions retain the ordinary invocation path and evaluation order.
        if a.kind==ActionKind::Change {
            let locals=(0..a.params.len()).map(|i|format!("const b{i}=inputs[{i}](p{i});")).collect::<String>();
            let argv=(0..a.params.len()).map(|i|format!(",b{i}")).collect::<String>();
            bound_factories.push(format!("(run,a,inputs,output,only,rt,begin,complete,abort,end)=>a.precondition?({})=>run(a,[{}],null,output,only,true):({})=>{{{locals}begin(a);try{{return complete(a,__action{i}(rt.ctx,rt{argv}),output,only);}}catch(e){{return abort(e);}}finally{{end();}}}}",params.join(","),captures.join(","),params.join(",")));
        }else{
            bound_factories.push(format!("(run,a,inputs,output,only)=>({})=>run(a,[{}],null,output,only,true)",params.join(","),captures.join(",")));
        }
        equal_results.push(format!("(a,b)=>{}",emitter.equality.expr(&a.result,"a","b")?));
        let mut env = Env::new();
        let mut vars = vec![];
        for (n, t) in &a.params {
            let v = emitter.fresh();
            env.insert(n.clone(), (v.clone(), t.clone()));
            vars.push(v)
        }
        let body = emitter.block(&a.body, &mut env, &a.result)?;
        let fallible=matches!(a.result,Type::Result(..));
        code.push_str(&format!("function __action{i}(ctx,rt{}){{__tick(ctx);{}{body}{}}}\n",if vars.is_empty(){String::new()}else{format!(",{}",vars.join(","))},if fallible{"try{"}else{""},if fallible{"}catch(e){if(e instanceof __Abort)return {Err:e.value};throw e;}"}else{""}));
        let summary=effects.action(&a.name).ok_or("missing action effects")?;
        let mut reads=summary.access.reads.clone();
        if let Some(guard)=p.contracts.precondition_checks.get(&a.name){reads.extend(effects.action(guard).ok_or("missing precondition effects")?.access.reads.clone());}
        let roots=p.states.iter().enumerate().filter(|(_,s)|reads.contains(&s.name)).map(|(i,_)|i).collect::<Vec<_>>();
        let single_cell=ink_core::action_shape::maximum_cell_writes(&checked_actions,i).is_some_and(|n|n<=1);
        let fresh_result=ink_core::action_shape::fresh_native_result(&checked_actions,i);
        actions.push(json!({"index":i,"name":a.name,"singleCell":single_cell,"freshResult":fresh_result,"roots":roots,"precondition":p.contracts.precondition_checks.get(&a.name),"change":a.kind==ActionKind::Change,"fallible":matches!(a.result,Type::Result(..)),"params":a.params.iter().map(|(n,t)|Ok(json!({"name":n,"type":descriptor(t,p,&mut BTreeSet::new())?}))).collect::<LangResult<Vec<_>>>()?,"result":descriptor(&a.result,p,&mut BTreeSet::new())?}));
    }
    code.push_str(&emitter.equality.source());
    code.push_str(&captures.source());
    code.push_str(&format!("const __stateQueryEquals=[{}];\n",equal_results.join(",")));
    code.push_str(&format!("const __stateRootEquals=[{}];\n",equal_roots.join(",")));
    code.push_str(&format!("const __stateBoundFactories=[{}];\n",bound_factories.join(",")));
    let roots=p.states.iter().map(|s|{let Type::Table(k,v)=&s.ty else{return Err("state root".into())};Ok(json!({"name":s.name,"key":descriptor(k,p,&mut BTreeSet::new())?,"value":descriptor(v,p,&mut BTreeSet::new())?,"snapshotKey":ink_core::snapshot_wire::descriptor(&layout.roots.iter().find(|(name,_,_)|name==&s.name).unwrap().1),"snapshotValue":ink_core::snapshot_wire::descriptor(&layout.roots.iter().find(|(name,_,_)|name==&s.name).unwrap().2)}))}).collect::<LangResult<Vec<_>>>()?;
    let events = p
        .events
        .iter()
        .map(|(n, t)| Ok(json!({"name":n,"type":descriptor(t,p,&mut BTreeSet::new())?,"snapshotType":ink_core::snapshot_wire::descriptor(&layout.events.iter().find(|(name,_)|name==n).unwrap().1)})))
        .collect::<LangResult<Vec<_>>>()?;
    let mut state = json!({"actions":actions,"roots":roots,"events":events,"keeps":keeps,"keepInvalidations":keep_invalidations,"invariantChecks":p.contracts.invariant_checks,"snapshot":{"program":layout.program,"schema":layout.schema}});
    super::value_layout::snapshots(p,&mut state,elisions)?;
    code.push_str(&format!(
        "const __stateMetadata={state};const __actions=[{}];const __stateInputCaptures=[{}];\n",
        (0..p.actions.len())
            .map(|i| format!("__action{i}"))
            .collect::<Vec<_>>()
            .join(","),
        capture_lists.join(",")
    ));
    code.push_str(
        "export function createState(){return __createState(__stateMetadata,__actions,__stateInputCaptures);}\n",
    );
    Ok((code, state))
}
