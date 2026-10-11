//! Standalone compiled representations. Proofs, source AST and checker stay in
//! the compiler; physical constructor values remain native between calls.
use ink_core::{logic::Sort, representation::CheckedRepresentation, syntax::Type, LangResult};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write,
};
struct Emit<'a> {
    checked: &'a CheckedRepresentation,
    types: BTreeMap<String, (usize, Type, Sort)>,
}
impl Emit<'_> {
    fn index(&self, t: &Type) -> LangResult<usize> {
        Ok(self
            .types
            .get(&serde_json::to_string(t).map_err(|e| e.to_string())?)
            .ok_or("missing representation codec")?
            .0)
    }
    fn read(&self, t: &Type, v: &str, d: &str) -> LangResult<String> {
        Ok(format!("read_{}({v},{d})", self.index(t)?))
    }
    fn read_mode(&self, t: &Type, v: &str, d: &str, mode: &str) -> LangResult<String> {
        Ok(format!("read_{}({v},{d},{mode})", self.index(t)?))
    }
    fn write(&self, t: &Type, v: &str, d: &str, s: &str) -> LangResult<String> {
        Ok(format!("write_{}({v},{d},{s})", self.index(t)?))
    }
    fn codecs(&self) -> LangResult<String> {
        let p = self.checked.module().program();
        let mut out = String::new();
        for (_, (index, t, sort)) in &self.types {
            if ink_core::local_collections::owned(t,p) {continue;}
            let read=match t{
    Type::U64=>"return v;".into(),
    Type::U32=>"return BigInt(v);".into(),
    Type::I32=>"return BigInt.asUintN(32,BigInt(v));".into(),
    Type::F32=>"if(internal)return BigInt(v);if(v&&typeof v==='object')return BigInt(v.F32Bits);__fpView.setFloat32(0,Number(v),true);return BigInt(__fpView.getUint32(0,true));".into(),
    Type::String=>"const bytes=__encoder.encode(v);let tail=__node(0,[]);for(let i=bytes.length-1;i>=0;i--)tail=__node(1,[BigInt(bytes[i]),tail]);return tail;".into(),
    Type::Named(n)if p.ids.contains(n)=>"const x=BigInt('0x'+v);return __node(0,[x>>64n,BigInt.asUintN(64,x)]);".into(),
    Type::Bool=>"return v;".into(),
    Type::Unit=>"return __node(0,[]);".into(),
    Type::Int=>"const text=v&&typeof v.Int==='string'?v.Int:typeof v==='bigint'?v.toString():'';const n=BigInt(text);let magnitude=n<0n?-n:n;if(magnitude===0n)return __node(0,[]);const bits=magnitude.toString(2);let tail=__node(0,[]);for(const bit of bits)tail=__node(bit==='0'?1:2,[tail]);return __node(n<0n?2:1,[tail]);".into(),
    Type::Named(n)if p.enums.contains_key(n)=>{let cases=p.enums[n].iter().enumerate().map(|(i,v)|format!("case {}:return __node({i},[]);",super::quote(&format!("{n}.{v}")))).collect::<String>();format!("switch(v){{{cases}default:return undefined;}}", )},
    Type::Named(n)if p.records.contains_key(n)=>{let fs=&p.records[n];let args=fs.iter().map(|(name,t)|self.read_mode(t,&format!("v[{}]",super::quote(name)),"d+1","internal")).collect::<LangResult<Vec<_>>>()?;format!("return __node(0,[{}]);",args.join(","))},
    Type::Option(a)=>format!("if(Object.hasOwn(v,'Some'))return __node(1,[{}]);return __node(0,[]);",self.read_mode(a,"v.Some","d+1","internal")?),
    Type::Result(a,b)=>format!("if(Object.hasOwn(v,'Ok'))return __node(0,[{}]);return __node(1,[{}]);",self.read_mode(a,"v.Ok","d+1","internal")?,self.read_mode(b,"v.Err","d+1","internal")?),
    Type::Vector(a,n)=>{let args=(0..*n).map(|i|self.read_mode(a,&format!("v[{i}]"),"d+1","internal")).collect::<LangResult<Vec<_>>>()?;format!("return __node(0,[{}]);",args.join(","))},
    Type::List(a)=>format!("let tail=__node(0,[]);for(let i=v.length-1;i>=0;i--)tail=__node(1,[{},tail]);return tail;",self.read_mode(a,"v[i]","d+1","internal")?),
    _=>return Err("unsupported standalone source codec".into()),
   };
            writeln!(out, "function read_{index}(v,d=0,internal=false){{{read}}}").unwrap();
            let write=match t{
    Type::F32=>"if(snapshot)return v;const x=__fpNumber(v);return !Number.isFinite(x)?{F32Bits:Number(v)}:x;".into(),
    Type::U64|Type::Bool=>"return v;".into(),Type::U32=>"return snapshot?v:Number(v);".into(),Type::I32=>"return snapshot?BigInt.asIntN(32,v):Number(BigInt.asIntN(32,v));".into(),Type::Unit=>"return null;".into(),
    Type::String=>"const bytes=[];let cursor=v;while(cursor.c!==0){bytes.push(Number(cursor.a[0]));cursor=cursor.a[1];}return new TextDecoder('utf-8',{fatal:true}).decode(Uint8Array.from(bytes));".into(),
    Type::Named(n)if p.ids.contains(n)=>"return ((v.a[0]<<64n)|v.a[1]).toString(16).padStart(32,'0');".into(),
    Type::Int=>"if(v.c===0)return snapshot?0n:{Int:'0'};let cursor=v.a[0],magnitude=0n,bits=0;while(cursor.c!==0){if(cursor.c===2)magnitude|=1n<<BigInt(bits);bits++;cursor=cursor.a[0];}const n=v.c===2?-magnitude:magnitude;return snapshot?n:{Int:n.toString()};".into(),
    Type::Named(n)if p.enums.contains_key(n)=>format!("return [{}][v.c];",p.enums[n].iter().map(|v|super::quote(&format!("{n}.{v}"))).collect::<Vec<_>>().join(",")),
    Type::Named(n)if p.records.contains_key(n)=>{let fs=p.records[n].iter().enumerate().map(|(i,(name,t))|Ok(format!("[{},{}]",super::quote(name),self.write(t,&format!("v.a[{i}]"),"d+1","snapshot")?))).collect::<LangResult<Vec<_>>>()?;format!("return Object.fromEntries([{}]);",fs.join(","))},
    Type::Option(a)=>format!("return v.c===0?{{None:null}}:{{Some:{}}};",self.write(a,"v.a[0]","d+1","snapshot")?),
    Type::Result(a,b)=>format!("return v.c===0?{{Ok:{}}}:{{Err:{}}};",self.write(a,"v.a[0]","d+1","snapshot")?,self.write(b,"v.a[0]","d+1","snapshot")?),
    Type::Vector(a,n)=>{let xs=(0..*n).map(|i|self.write(a,&format!("v.a[{i}]"),"d+1","snapshot")).collect::<LangResult<Vec<_>>>()?;format!("return [{}];",xs.join(","))},
    Type::List(a)=>format!("let cursor=v;const xs=[];while(cursor.c!==0){{xs.push({});cursor=cursor.a[1];}}return xs;",self.write(a,"cursor.a[0]","d+1","snapshot")?),_=>unreachable!(),
   };
            writeln!(
                out,
                "function write_{index}(v,d=0,snapshot=false){{{write}}}"
            )
            .unwrap();
            // Data nodes carry native tags/fields; type-specific inspection is emitted
            // from the checked declarations, not a runtime type or definition image.
            let _ = sort;
        }
        Ok(out)
    }
}
pub fn lower(checked: &CheckedRepresentation) -> LangResult<String> {
    let executable = checked.definition().reachable_executable_declarations()?;
    let types = checked
        .projection()
        .types
        .iter()
        .filter(|(_, sort)| match sort {
            Sort::Data(id) => executable.contains_key(id),
            _ => true,
        })
        .enumerate()
        .map(|(i, (key, sort))| {
            Ok((
                key.clone(),
                (
                    i,
                    serde_json::from_str::<Type>(key).map_err(|e| e.to_string())?,
                    sort.clone(),
                ),
            ))
        })
        .collect::<LangResult<BTreeMap<_, _>>>()?;
    let e = Emit { checked, types };
    let p = checked.module().program();
    let plan = checked.plan();
    let projection = checked.projection();
    let (mut queries, observers) = ink_core::field_projection::host_observers(checked)?;
    queries.extend( super::representation_query::declarations(checked)?);
    let transitions = super::representation_transition::declarations(checked)?;
    queries.extend(transitions.clone());
    let mut out = super::definition_literal::lower(checked.definition(), &queries)?;
    out.push_str("const __encoder=new TextEncoder();\nfunction __keyCompare(a,b){if(typeof a==='string'&&typeof b==='string'){const x=__encoder.encode(a),y=__encoder.encode(b);for(let i=0;i<Math.min(x.length,y.length);i++)if(x[i]!==y[i])return x[i]<y[i]?-1:1;return x.length-y.length;}return a<b?-1:a>b?1:0;}\n\n");
    out.push_str(include_str!("snapshot.mjs"));
    out.push_str(&e.codecs()?);
    let layout = ink_core::snapshot::layout(p)?;
    let roots=p.states.iter().map(|s|{let Type::Table(k,v)=&s.ty else{return Err("representation state root".into())};Ok(serde_json::json!({"name":s.name,"key":super::descriptor(k,p,&mut BTreeSet::new())?,"value":super::descriptor(v,p,&mut BTreeSet::new())?}))}).collect::<LangResult<Vec<_>>>()?;
    let events = p
        .events
        .iter()
        .map(|(n, t)| {
            Ok(serde_json::json!({"name":n,"type":super::descriptor(t,p,&mut BTreeSet::new())?}))
        })
        .collect::<LangResult<Vec<_>>>()?;
    writeln!(out,"const __meta={};",serde_json::json!({"roots":roots,"events":events,"snapshot":{"program":layout.program,"schema":layout.schema}})).unwrap();
    out.push_str("function __fromTables(tables){const roots=[];\n");
    for (i, t) in projection.tables.iter().enumerate() {
        writeln!(out,"{{const rows=[...tables[{i}].entries()];let prior,tail=__node(0,[]);for(let j=rows.length-1;j>=0;j--){{const [key,value]=rows[j];tail=__node(1,[{},{},tail]);}}roots.push(tail);}}",e.read_mode(&t.key_type,"key","0","true")?,e.read_mode(&t.value_type,"value","0","true")?).unwrap();
    }
    out.push_str("return __node(0,roots);}\nfunction __tables(logical){const tables=[];\n");
    for (i, t) in projection.tables.iter().enumerate() {
        writeln!(out,"{{let cursor=logical.a[{i}],prior;const rows=new Map();while(cursor.c!==0){{const [key,value,tail]=cursor.a;rows.set({},{});cursor=tail;}}tables.push(rows);}}",e.write(&t.key_type,"key","0","true")?,e.write(&t.value_type,"value","0","true")?).unwrap();
    }
    out.push_str("return tables;}\nfunction __events(list){const events=[];let cursor=list;while(cursor.c!==0){const [event,tail]=cursor.a;switch(event.c){\n");
    for (i, (name, t)) in p.events.iter().enumerate() {
        writeln!(
            out,
            "case {i}:events.push([{}, {}, {}]);break;",
            super::quote(name),
            e.write(t, "event.a[0]", "0", "true")?,
            e.write(t, "event.a[0]", "0", "false")?
        )
        .unwrap();
    }
    out.push_str("default:throw Error('unknown source event');}cursor=tail;}return events;}\n");
    out.push_str("// CALLER CONTRACT: arguments must have declared types and canonical values.\n// Malformed data may throw, produce wrong results or corrupt state.\nexport function createState(){let version=0n,outbox=[],busy=false,transaction=null;\n");
    writeln!(
        out,
        "const initial=__node(0,[{}]);let physical=f_{}(initial);",
        projection
            .tables
            .iter()
            .map(|_| "__node(0,[])")
            .collect::<Vec<_>>()
            .join(","),
        plan.encode
    )
    .unwrap();
    out.push_str("function boundary(){if(busy)throw Error('state operation already in progress');}\nasync function invoke_json(name,args){boundary();switch(name){\n");
    for source in &p.actions {
        let a = &plan.actions[&source.name];
        writeln!(out, "case {}:{{", super::quote(&source.name)).unwrap();
        let mut args = vec!["physical".into(), "version".into()];
        for (i, (_, t)) in source.params.iter().enumerate() {
            writeln!(
                out,
                "const a{i}={};",
                e.read(t, &format!("args[{i}]"), "0")?
            )
            .unwrap();
            args.push(format!("a{i}"));
        }
        let query = format!("q_{}", a.execute);
        if queries.contains_key(&query) {
            writeln!(out,"const reply=f_{query}({});return {{result:{},committed:false,version,events:[]}};}}",args.join(","),e.write(&source.result,"reply","0","false")?).unwrap();
            continue;
        }
        writeln!(out,"const executed=f_{}({});const observed=f_{}(executed),next=f_{}(executed);if(observed.c===1)throw Error('commit sequence exhausted');const [reply,committed,newVersion,,events]=observed.a;const result={},staged=__events(events),published=[];",if transitions.contains_key(&format!("t_{}",a.execute)) {format!("t_{}",a.execute)}else{a.execute.clone()},args.join(","),observers[&source.name],a.next,e.write(&source.result,"reply","0","false")?).unwrap();
        out.push_str("if(committed){for(let i=0;i<staged.length;i++){const [channel,value,publicValue]=staged[i];outbox.push({commit:newVersion,position:BigInt(i),channel,value});published.push({commit:newVersion,position:BigInt(i),channel,value:publicValue});}}physical=next;version=newVersion;return {result,committed,version,events:published};}\n");
    }
    out.push_str("default:throw Error(`unknown action ${name}`);}}\nreturn Object.freeze({invoke:invoke_json,invoke_json,beginTransaction(){boundary();if(transaction)throw Error('state transaction already in progress');transaction={physical,version,outbox,length:outbox.length};return true;},commitTransaction(){boundary();if(!transaction)throw Error('no state transaction');transaction=null;},rollbackTransaction(){boundary();if(!transaction)throw Error('no state transaction');physical=transaction.physical;version=transaction.version;outbox=transaction.outbox;outbox.length=transaction.length;transaction=null;},");

    out.push_str("get version(){return version;},get outbox(){return outbox.map(e=>({...e,value:__publicEvent(e.channel,e.value)}));},acknowledge_through(commit,position){boundary();commit=BigInt(commit);position=BigInt(position);outbox=outbox.filter(e=>e.commit>commit||(e.commit===commit&&e.position>position));},async checkpoint(limits){boundary();\n");
    writeln!(out,"const logical=f_{}(physical);return __snapshotEncode(__meta,{{version,tables:__tables(logical),outbox}},limits);}},async restore(bytes,limits){{boundary();if(transaction)throw Error('restore during state transaction');busy=true;try{{const saved=await __snapshotDecode(__meta,bytes,limits);const logical=__fromTables(saved.tables);const next=f_{}(logical);physical=next;version=saved.version;outbox=saved.outbox;}}finally{{busy=false;}}}}}});}}",plan.decode,plan.encode).unwrap();
    out.push_str("function __publicEvent(channel,value){switch(channel){\n");
    for (name, t) in &p.events {
        writeln!(
            out,
            "case {}:return {};",
            super::quote(name),
            e.write(t, &e.read_mode(t, "value", "0", "true")?, "0", "false")?
        )
        .unwrap();
    }
    out.push_str("default:throw Error('unknown source event');}}\n");
    if out.len() > 16_000_000 {
        return Err("standalone representation source size limit".into());
    }
    Ok(out)
}
