use ink_core::{core::CheckedModule,eval,stateful::Runtime,syntax::parse};
use serde_json::{json,Value};
use std::{fs,process::Command};

#[test]
fn owned_record_locals_preserve_moves_loans_shared_lists_and_transactions(){
    let mut source=include_str!("fixtures/owned-records.ink").to_owned();
    source+=&format!("fn wide(s:Scratch)->Scratch{{let xs:List<f32>=[{}];return Scratch{{left:s.left.push(xs.at_or(9,0.0)),right:s.right,history:s.history,total:s.total}};}}",vec!["0.0";530].join(","));
    source+="fn fallback(xs:List<f32>,ys:List<f32>,w:f32,n:u32)->Out{let s=Scratch{left:buffer(xs),right:buffer(ys),history:xs,total:w};return finish(wide(s),xs);}";
    let directory=std::env::temp_dir().join(format!("ink-owned-records-{}",std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let bits=[0u32,0x80000000,1,0x80000001,0x007fffff,0x00800000,0x3f800001,0xbf800002,
        0x7f7fffff,0xff7fffff,0x7f800000,0xff800000,0x7f800001,0x7fc12345,0xffc12345,0x4b800000];
    for carrier in ["native","bits"]{
        let program_source=if carrier=="bits"{format!("{source}\ntrainable state Rows:Table<u32,List<f32>>=Table.empty();event computed:Out;
learn change seed(xs:List<f32>)->Unit writes(Rows){{Rows.put(0,xs);return ();}}
adaptive query read(xs:List<f32>,w:f32,n:u32)->Out reads(Rows){{return run(xs,Rows.get(0).unwrap_or([]),w,n);}}
learn change fit(xs:List<f32>,w:f32,n:u32,fail:Bool)->Result<Out,String> reads(Rows) writes(Rows) emits(computed){{let result=run(xs,Rows.get(0).unwrap_or([]),w,n);Rows.put(0,result.left);emit computed(result);if fail{{return Err(\"rollback\");}}return Ok(result);}}")}else{source.clone()};
        let module=CheckedModule::from_source(parse(&program_source).unwrap()).unwrap();
        let mut cases=vec![];
        let mut add=|name:&str,args:Value|{
            let f=module.program().functions.iter().find(|f|f.name==name).unwrap();
            let values=args.as_array().unwrap().iter().zip(&f.params).map(|(v,(_,t))|eval::Value::from_program_json(v,t,module.program()).unwrap()).collect();
            let expected=eval::call_checked(&module,name,values,&mut 1_000_000).unwrap().json();
            cases.push(json!({"name":name,"args":args,"expected":expected}));
        };
        for(i,b)in bits.iter().enumerate(){
            let x=json!({"F32Bits":b});let y=json!({"F32Bits":bits[(i+3)%bits.len()]});
            for n in [0,1,3,6]{for xs in [json!([]),json!([x,y]),json!([y,x,0.5,1.5,-2.0,3.0])]{
                for name in ["run","until","reordered","fallback","indexed","folded_left","folded_right","folded_until","nested"]{add(name,json!([xs,[y,x],x,n]));}
            }}
            if i<3{add("parallel",json!([[x,y],x,3]));}
        }
        assert_eq!(cases.len(),1731);
        let mut history=vec![];
        if carrier=="bits"{
            let mut runtime=Runtime::new(module.program().clone()).unwrap();
            history.push(json!({"restore":runtime.checkpoint_portable().unwrap()}));
            for b in bits{
                let x=json!({"F32Bits":b});
                for(name,args)in[("seed",json!([[x,0.5]])),("read",json!([[x,1.5],0.75,3])),
                    ("fit",json!([[x,1.5],0.75,3,false])),("read",json!([[x,1.5],0.75,6])),
                    ("fit",json!([[x,1.5],0.75,3,true])),("read",json!([[x,1.5],0.75,1]))]{
                    let before=runtime.checkpoint_portable().unwrap();let result=runtime.invoke_json(name,&args).unwrap().json();
                    let snapshot=runtime.checkpoint_portable().unwrap();
                    if name=="fit"&&args[3]==true{assert_eq!(before,snapshot);}
                    history.push(json!({"call":name,"args":args,"expected":result,"snapshot":snapshot}));
                }
                let snapshot=runtime.checkpoint_portable().unwrap();history.push(json!({"restore":snapshot}));
                runtime=Runtime::restore_portable_checked(&module,&snapshot).unwrap();
            }
        }
        let emitted=ink_lowering_js::lower(&module).unwrap();
        fs::write(directory.join("program.mjs"),&emitted.javascript).unwrap();
        // Test-only source probes establish both expansion and its bounded
        // fallback. They are never shipped or accepted as semantic evidence.
        let mut instrumented=emitted.javascript;
        for(name,probe)in[("advance","__ownedAdvance"),("wide","__ownedWide")]{
            let index=module.program().functions.iter().position(|f|f.name==name).unwrap();
            let start=instrumented.find(&format!("function __fn{index}(" )).unwrap();
            let body=start+instrumented[start..].find("{\n").unwrap()+2;
            instrumented.insert_str(body,&format!("globalThis.{probe}=(globalThis.{probe}??0)+1;\n"));
        }
        fs::write(directory.join("instrumented.mjs"),instrumented).unwrap();
        fs::write(directory.join("cases.json"),serde_json::to_vec(&cases).unwrap()).unwrap();
        fs::write(directory.join("history.json"),serde_json::to_vec(&history).unwrap()).unwrap();
        fs::write(directory.join("test.mjs"),r#"
import assert from 'node:assert/strict';import fs from 'node:fs';import * as m from './program.mjs';
const read=name=>JSON.parse(fs.readFileSync(new URL(name,import.meta.url))),cases=read('cases.json');
for(const c of cases){
 const before=structuredClone(c.args);assert.deepEqual(m.call(c.name,c.args),c.expected,c.name);assert.deepEqual(c.args,before,'borrowed inputs');
 const entry=m.compiledValues[c.name],args=c.args.map((v,i)=>entry.params[i].capture(v)),expected=m.valueCodecs[c.name].result.encode(c.expected);
 assert.deepEqual(entry.result.encode(m.callCompiled(c.name,args)),expected,c.name);
 if(c.name!=='parallel')for(const fixed of [[],[0],[0,2],[0,1,2,3]]){
  const token=m.prepareCompiled(c.name,fixed.map(i=>[i,args[i]]));
  for(let attempt=0;attempt<2;attempt++){
   const out=m.callPreparedCompiled(token,args.filter((_,i)=>!fixed.includes(i)));
   assert.deepEqual(entry.result.encode(out),expected,c.name);
   const host=entry.result.materialize(out);host.left.splice(0);host.right.splice(0);host.history.splice(0);host.source.splice(0);
  }
 }
}
assert.ok(m.parallelStatistics().workers>=6);assert.ok(m.parallelStatistics().lastWorkerIds.length>=2);
for(const name of ['advance','finish','identity','wide','index_step','nested_finish']){assert.equal(m.functions[name],undefined);assert.throws(()=>m.call(name,[]),/temporary storage/);}
const q=await import('./instrumented.mjs');globalThis.__ownedAdvance=0;globalThis.__ownedWide=0;
assert.deepEqual(q.functions.run([1.5,-0],[0.25],0.5,3),m.functions.run([1.5,-0],[0.25],0.5,3));assert.equal(globalThis.__ownedAdvance,0);
for(const name of ['folded_left','folded_right','folded_until']){assert.deepEqual(q.functions[name]([1.5,-0],[0.25],0.5,3),m.functions[name]([1.5,-0],[0.25],0.5,3));assert.equal(globalThis.__ownedAdvance,0);}
assert.deepEqual(q.functions.fallback([1.5,-0],[0.25],0.5,3),m.functions.fallback([1.5,-0],[0.25],0.5,3));assert.equal(globalThis.__ownedWide,1);
delete globalThis.__ownedAdvance;delete globalThis.__ownedWide;
const trace=[];let fail=true;const borrowed={get a(){trace.push('a');return 7;},get b(){trace.push('b');if(fail)throw Error('borrowed retry');return 9;}};
assert.equal(m.functions.borrowed_fold([],borrowed),7);assert.deepEqual(trace,['a']);trace.length=0;
assert.throws(()=>m.functions.borrowed_fold([3],borrowed),/borrowed retry/);assert.deepEqual(trace,['b']);trace.length=0;
fail=false;assert.equal(m.functions.borrowed_fold([3],borrowed),3);assert.deepEqual(trace,['b']);trace.length=0;
assert.equal(m.functions.borrowed_stop([3,4],borrowed),3);assert.deepEqual(trace,['a','b']);
const normalize=v=>typeof v==='bigint'?String(v):typeof v==='number'&&Number.isInteger(v)?(Object.is(v,-0)?'negative zero':String(v)):Array.isArray(v)?v.map(normalize):v&&typeof v==='object'?Object.fromEntries(Object.entries(v).map(([k,x])=>[k,normalize(x)])):v;
let outcomes=0,restores=0;
const history=read('history.json');
if(history.length){const state=m.createState();for(const step of history){
 if(step.restore){await state.restore(step.restore);restores++;continue;}
 const actual=state.invoke(step.call,step.args);assert.deepEqual(normalize(actual),normalize(step.expected));
 if(actual.result?.Ok?.left)actual.result.Ok.left.splice(0);
 assert.deepEqual(Array.from(await state.checkpoint()),step.snapshot);outcomes++;
}const before=await state.checkpoint();state.beginTransaction();state.invoke('fit',[[1.5,-0],0.5,3,false]);state.rollbackTransaction();assert.deepEqual(await state.checkpoint(),before);}
console.log(JSON.stringify({status:'passed',values:cases.length,outcomes,restores,actualWorkers:true,preparedReuse:true,detachedOutputs:true}));
"#).unwrap();
        let result=Command::new("node").env("INK_THREADS","2").arg(directory.join("test.mjs")).output().unwrap();
        assert!(result.status.success(),"{carrier}: {}{}",String::from_utf8_lossy(&result.stdout),String::from_utf8_lossy(&result.stderr));
        println!("{carrier}: {}",String::from_utf8_lossy(&result.stdout));
        if let Some(destination)=std::env::var_os("INK_OWNED_RECORD_ARTIFACTS"){
            let destination=std::path::PathBuf::from(destination).join(carrier);fs::create_dir_all(&destination).unwrap();
            for name in ["program.mjs","instrumented.mjs","cases.json","history.json","test.mjs"]{fs::copy(directory.join(name),destination.join(name)).unwrap();}
            fs::write(destination.join("source.ink"),program_source).unwrap();fs::write(destination.join("source.core.json"),module.bytes().unwrap()).unwrap();
            fs::write(destination.join("observations.json"),result.stdout).unwrap();
        }
    }
    fs::remove_dir_all(directory).unwrap();
}
