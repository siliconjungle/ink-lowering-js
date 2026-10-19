use ink_core::{core::CheckedModule, stateful::Runtime, syntax::parse};
use serde_json::json;
use std::{fs,process::Command};

#[test]
fn native_helpers_keep_exact_state_events_snapshots_ownership_and_transactions() {
    let source=r#"module StateFloat;
record Packet {x:f32,values:List<f32>,pair:Vec2<f32>,maybe:Option<f32>,outcome:Result<f32,Option<f32>>,wide:f64,}
state Rows:Table<u32,Packet>=Table.empty();
event saved:Packet;
event scalar:f32;
fn copy(p:Packet)->Packet{return p;}
fn arithmetic(x:f32,y:f32)->f32{let a=x*y;let b=a+x;return (b-a)*y;}
fn parallel(xs:List<f32>)->List<f32>{return parallel_map(xs,1,fn(i)=>fn(x)=>-x);}
fn device(xs:List<f32>)->List<f32>{return xs.map(fn(x)=>x*2.0+1.0);}
keep cached:List<Packet>=Rows.values().map(fn(p)=>copy(p));
query read()->List<Packet> reads(cached){return cached;}
query direct(x:f32)->f32{return x;}
query via(x:f32,y:f32)->f32{return arithmetic(x,y);}
query workers(xs:List<f32>)->List<f32>{return parallel(xs);}
change put(p:Packet,fail:Bool)->Result<Packet,String> reads(cached) writes(Rows) emits(saved){
 let value=copy(p);Rows.put(0,value);let seen=read();emit saved(copy(value));
 if fail{return Err("rollback");}return Ok(copy(value));
}
change remember(x:f32)->f32 emits(scalar){emit scalar(x);return arithmetic(x,1.0);}
"#;
    let module=CheckedModule::from_source(parse(source).unwrap()).unwrap();
    let emitted=ink_lowering_js::lower(&module).unwrap();
    assert_eq!(emitted.manifest["f32_storage"],"exact-u32-bits");
    assert_eq!(emitted.manifest["f32_computation"],"native-number-with-exact-nonfinite-tags");
    assert!(emitted.javascript.contains("function __stateFn"));
    let mut runtime=Runtime::from_checked(&module).unwrap();
    let mut bits=vec![0u32,0x80000000,1,0x80000001,0x007fffff,0x00800000,0x7f7fffff,0xff7fffff,0x7f800000,0xff800000,0x7f800001,0x7fc12345,0xffc12345];
    let mut seed=83u32;for _ in 0..16{seed=seed.wrapping_mul(1664525).wrapping_add(1013904223);bits.push(seed);}
    let mut history=vec![];
    for (i,b) in bits.iter().enumerate(){
        let a=json!({"F32Bits":b});let other=json!({"F32Bits":bits[(i+3)%bits.len()]});
        let p=json!({"x":a,"values":[a,other],"pair":[a,other],"maybe":if i%2==0{json!({"Some":a})}else{json!({"None":null})},"outcome":if i%3==0{json!({"Ok":a})}else{json!({"Err":{"Some":other}})},"wide":{"F64Bits":if i%2==0{"9223372036854775808"}else{"9221120237041095220"}}});
        for (name,args) in [("put",json!([p,false])),("read",json!([])),("put",json!([p,true])),("read",json!([])),("direct",json!([a])),("via",json!([a,other])),("remember",json!([a]))]{
            let before=runtime.checkpoint_portable().unwrap();
            let reply=runtime.invoke_json(name,&args).unwrap().json();
            let snapshot=runtime.checkpoint_portable().unwrap();
            if name=="put"&&args[1]==true{assert_eq!(snapshot,before);}
            history.push(json!({"name":name,"args":args,"reply":reply,"snapshot":snapshot}));
        }
    }
    let xs=json!([{"F32Bits":0x80000000u32},{"F32Bits":0x7f800001u32},1.5,-2.0]);
    let args=json!([xs]);let reply=runtime.invoke_json("workers",&args).unwrap().json();
    history.push(json!({"name":"workers","args":args,"reply":reply,"snapshot":runtime.checkpoint_portable().unwrap()}));
    let root=std::env::temp_dir().join(format!("ink-state-native-float-{}",std::process::id()));fs::create_dir_all(&root).unwrap();
    fs::write(root.join("program.mjs"),emitted.javascript).unwrap();
    fs::write(root.join("history.json"),serde_json::to_vec(&history).unwrap()).unwrap();
    fs::write(root.join("run.mjs"),r#"import assert from 'node:assert/strict';import fs from 'node:fs';import * as m from './program.mjs';
const history=JSON.parse(fs.readFileSync(new URL('history.json',import.meta.url)));
const normalize=v=>typeof v==='bigint'?String(v):typeof v==='number'&&Number.isInteger(v)?(Object.is(v,-0)?'negative zero':String(v)):Array.isArray(v)?v.map(normalize):v&&typeof v==='object'?Object.fromEntries(Object.entries(v).map(([k,x])=>[k,normalize(x)])):v;
// Captured stateful-module pure entries normalize all float descendants before
// entering the helper. Borrowed pure-only entries keep their separate contract.
const input=structuredClone(history[0].args[0]);input.x=1+2**-25;input.values=[.1,-.1];input.pair=[2**24+1,-(2**-150)];
const output=m.functions.copy(input);assert.equal(output.x,Math.fround(input.x));assert.deepEqual(output.values,input.values.map(Math.fround));assert.deepEqual(output.pair,input.pair.map(Math.fround));
let reads=0;const tagged={get F32Bits(){reads++;return 0x7f812345;}};input.x=tagged;assert.deepEqual(m.functions.copy(input).x,{F32Bits:0x7f812345});assert.equal(reads,1);
assert.deepEqual(m.functions.device([-1,0,1]),[-1,1,3]);
for(const mode of ['invoke','prepared','bound']){
 const state=m.createState(),bindings=new Map();
 for(const step of history){
  if(!bindings.has(step.name))bindings.set(step.name,state.bind(step.name));
  const args=structuredClone(step.args);
  const reply=mode==='prepared'?state.invokePrepared(state.prepare(step.name,args)):mode==='bound'?bindings.get(step.name)(...args):state.invoke(step.name,args);
  assert.deepEqual(normalize(reply),normalize(step.reply),`${mode}:${step.name}`);
  assert.deepEqual(args,step.args);
  assert.deepEqual([...await state.checkpoint()],step.snapshot);
  if(step.name==='read'&&reply.result.length){reply.result[0].values[0]=123;reply.result[0].x=123;}
 }
 const before=await state.checkpoint();state.beginTransaction();bindings.get('remember')(-0);state.rollbackTransaction();assert.deepEqual(await state.checkpoint(),before);
 const restored=m.createState();await restored.restore(before);assert.deepEqual(await restored.checkpoint(),before);
 const step=history.at(-1);assert.deepEqual(normalize(await restored.invokeAsync(step.name,step.args)),normalize(step.reply));assert.deepEqual(await restored.checkpoint(),before);
 const events=state.outbox;events[0].value.values[0]=123;assert.deepEqual(await state.checkpoint(),before);
}
console.log(JSON.stringify({histories:3,steps:history.length,referenceSnapshots:true,prepared:true,bound:true,hostRollback:true,workerState:true,detached:true}));
"#).unwrap();
    let result=Command::new("node").env("INK_THREADS","2").arg(root.join("run.mjs")).output().unwrap();
    if let Some(output)=std::env::var_os("INK_STATE_FLOAT_ARTIFACTS"){
        let output=std::path::PathBuf::from(output);fs::create_dir_all(&output).unwrap();
        for name in ["program.mjs","history.json","run.mjs"]{fs::copy(root.join(name),output.join(name)).unwrap();}
        fs::write(output.join("source.ink"),source).unwrap();fs::write(output.join("source.core.json"),module.bytes().unwrap()).unwrap();fs::write(output.join("observations.json"),&result.stdout).unwrap();
    }
    fs::remove_dir_all(root).unwrap();
    assert!(result.status.success(),"{}{}",String::from_utf8_lossy(&result.stdout),String::from_utf8_lossy(&result.stderr));
    println!("{}",String::from_utf8_lossy(&result.stdout));
}

#[test]
fn recursive_float_shapes_keep_the_existing_bit_lane() {
    let module=CheckedModule::from_source(parse("module RecursiveFloat;record Tree{x:f32,children:List<Tree>,}fn copy(t:Tree)->Tree{return t;}state Count:u32=0;").unwrap()).unwrap();
    let emitted=ink_lowering_js::lower(&module).unwrap();
    assert_eq!(emitted.manifest["f32_computation"],"exact-u32-bits");
    assert!(!emitted.javascript.contains("function __stateFn"));
}
