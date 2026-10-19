//! Private, typed bridges between exact-bit state and native F32 helpers.
//! No source arithmetic, ownership transfer or public codec changes occur here.
use super::{descriptor, quote, sum_layout::Sums};
use ink_core::{syntax::{Program, Type}, LangResult};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct Bridges<'a> {
    program: &'a Program,
    elisions: &'a BTreeSet<String>,
    sums: &'a Sums,
    conversions: BTreeMap<String, (String, String)>,
    wrappers: String,
    functions: BTreeSet<usize>,
}

impl<'a> Bridges<'a> {
    /// Optional literal representation support. Unsupported ownership/recursive
    /// float shapes or exhausted schema/source budgets keep the old bit lane.
    pub fn prepare(program: &'a Program, elisions: &'a BTreeSet<String>, sums: &'a Sums) -> Option<Self> {
        let mut bridge=Self {program,elisions,sums,conversions:BTreeMap::new(),wrappers:String::new(),functions:BTreeSet::new()};
        for (index,f) in program.functions.iter().enumerate() {
            let params=(0..f.params.len()).map(|i|format!("a{i}")).collect::<Vec<_>>();
            let args=f.params.iter().zip(&params).map(|((_,ty),arg)|bridge.convert(ty,arg,true)).collect::<LangResult<Vec<_>>>().ok()?;
            let call=format!("__fn{index}({})",args.join(","));
            let result=bridge.convert(&f.result,&call,false).ok()?;
            if args==params && result==call {continue;}
            bridge.functions.insert(index);
            bridge.wrappers.push_str(&format!("function __stateFn{index}({}){{return {result};}}\n",params.join(",")));
            if bridge.source().len()>262_144 {return None;}
        }
        Some(bridge)
    }

    pub fn source(&self) -> String {
        self.conversions.values().map(|(_,code)|code.as_str()).collect::<String>()+&self.wrappers
    }

    pub fn functions(&self) -> BTreeSet<usize> {self.functions.clone()}

    fn has_float(&self, ty: &Type, seen: &mut BTreeSet<String>) -> bool {
        match ty {
            Type::F32=>true,
            Type::Named(n) if seen.insert(n.clone())=>self.program.records.get(n).is_some_and(|fields|fields.iter().any(|(_,t)|self.has_float(t,seen))),
            Type::List(t)|Type::Vector(t,_)|Type::Option(t)|Type::Buffer(t)=>self.has_float(t,seen),
            Type::Result(a,b)|Type::Map(a,b)|Type::Table(a,b)=>self.has_float(a,seen)||self.has_float(b,seen),
            _=>false,
        }
    }

    fn convert(&mut self, ty: &Type, value: &str, native: bool) -> LangResult<String> {
        if !self.has_float(ty,&mut BTreeSet::new()) {return Ok(value.into());}
        if *ty==Type::F32 {return Ok(format!("{}({value})",if native {"__nfFromBits"}else{"__nfBits"}));}
        let key=serde_json::to_string(&(ty,native)).map_err(|e|e.to_string())?;
        if let Some((name,code))=self.conversions.get(&key) {
            if code.is_empty() {return Err("recursive state float bridge retains bit execution".into());}
            return Ok(format!("{name}({value})"));
        }
        if self.conversions.len()>=4096 {return Err("state float bridge schema limit".into());}
        let name=format!("__stateFloat{}",self.conversions.len());
        self.conversions.insert(key.clone(),(name.clone(),String::new()));
        let body=match ty {
            Type::Named(n)=>{
                let fields=self.program.records.get(n).ok_or("state float record")?.clone();
                if self.elisions.contains(n) {
                    self.convert(&fields[0].1,"value",native)?
                } else {
                    let fields=fields.iter().map(|(field,t)|Ok(format!("[{}]:{}",quote(field),self.convert(t,&format!("value[{}]",quote(field)),native)?))).collect::<LangResult<Vec<_>>>()?;
                    format!("({{{}}})",fields.join(","))
                }
            }
            Type::List(t)|Type::Vector(t,_)=>format!("Array.from(value,item=>{})",self.convert(t,"item",native)?),
            Type::Option(t)=>format!("Object.hasOwn(value,'Some')?{{Some:{}}}:{{None:null}}",self.convert(t,"value.Some",native)?),
            Type::Result(a,b)=>{
                let schema=descriptor(ty,self.program,&mut BTreeSet::new())?;
                let view=self.sums.view(&schema,"value");
                let ok=self.convert(a,"branch.Ok",native)?;
                let err=self.convert(b,"branch.Err",native)?;
                let ok=self.sums.make(&schema,"Ok",&ok);
                let err=self.sums.make(&schema,"Err",&err);
                format!("(()=>{{const branch={view};return Object.hasOwn(branch,'Ok')?{ok}:{err};}})()")
            }
            // Converting an owned container would require a checked loan/move
            // bridge. Keep the established representation instead of copying or
            // mutating its private handle behind the ownership analysis.
            _=>return Err("owned state float bridge retains bit execution".into()),
        };
        self.conversions.insert(key,(name.clone(),format!("function {name}(value){{return {body};}}\n")));
        Ok(format!("{name}({value})"))
    }
}
