use ink_core::{core::CheckedModule, eval, syntax::parse};
use serde_json::{json, Value};
use std::{fs, process::Command};

#[test]
fn sealed_partial_calls_retain_only_stable_readonly_values_in_source_order() {
 let source=include_str!("fixtures/prepared_residual.ink");
 let module=CheckedModule::from_source(parse(source).unwrap()).unwrap();
 let mut cases=Vec::<Value>::new();
 for (name,args) in [
  ("indexed",json!([[1,2,3],[0,1,2,7]])),("guarded",json!([[1,2,3],[]])),
  ("guarded",json!([[1,2,3],[7]])),("guarded",json!([[1,2,3],[0,1,2,7]])),
  ("edited",json!([[1,2,3],[7,2,9]])),("edited",json!([[1,2,3],[9,7,11]])),
  ("transformed",json!([[1,2,4294967295u32],3])),("transformed",json!([[1,2,4294967295u32],7])),
  ("shadowed",json!([[1,2,3],[8,9]])),("shadowed",json!([[1,2,3],[17]])),
  ("floating",json!([[0.0,-0.0,{"F32Bits":0x7fc12345u32},{"F32Bits":1}],true])),
  ("floating",json!([[0.0,-0.0,{"F32Bits":0x7fc12345u32},{"F32Bits":1}],false])),
  ("checked",json!([[1,2,3],[0,1,2,7],true])),("plain",json!([u32::MAX])),
 ]{
  let f=module.program().functions.iter().find(|f|f.name==name).unwrap();
  let input=args.as_array().unwrap().iter().zip(&f.params).map(|(v,(_,t))|eval::Value::from_program_json(v,t,module.program()).unwrap()).collect();
  let expected=eval::call(module.program(),name,input,&mut 1000000).unwrap().json();
  cases.push(json!({"name":name,"args":args,"expected":expected}));
 }
 let code=ink_lowering_js::lower(&module).unwrap().javascript;
 let root=std::env::temp_dir().join(format!("ink-prepared-residual-{}",std::process::id()));fs::create_dir_all(&root).unwrap();
 fs::write(root.join("program.mjs"),code).unwrap();fs::write(root.join("cases.json"),serde_json::to_vec(&cases).unwrap()).unwrap();
 fs::write(root.join("test.mjs"),r#"
import assert from 'node:assert/strict';import fs from 'node:fs';import * as m from './program.mjs';
const cases=JSON.parse(fs.readFileSync(new URL('cases.json',import.meta.url))),codecs=m.compiledValues;
for(const name of Object.keys(codecs)){void codecs[name].params;void codecs[name].result;}
const materialize=(name,h)=>codecs[name].result.materialize(h);
let calls=0;
for(const c of cases){
 const entry=codecs[c.name],handles=c.args.map((v,i)=>entry.params[i].capture(v));
 for(const indices of [[],[0],handles.map((_,i)=>i),handles.map((_,i)=>i).reverse()]){
  const unique=[...new Set(indices)];const bindings=unique.map(i=>[i,handles[i]]);
  const prepared=m.prepareCompiled(c.name,bindings),remaining=handles.filter((_,i)=>!unique.includes(i));
  bindings.splice(0,bindings.length); // caller-owned configuration is captured
  const expected=m.valueCodecs[c.name].result.encode(c.expected);
  for(let n=0;n<3;n++)assert.deepEqual(entry.result.encode(m.callPreparedCompiled(prepared,remaining)),expected,c.name);
  assert.deepEqual(entry.result.encode(m.callCompiled(c.name,handles)),expected);calls+=3;
 }
}
// Reuse the same captured keys while query inputs change. Host mutation and
// materialized output mutation must not change retained private values.
const host=[1,2,3],key=codecs.indexed.params[0].capture(host);host[0]=99;
const query=v=>codecs.indexed.params[1].capture(v),one=query([0,1,2,7]),two=query([7,2,9]),empty=query([]);
const token=m.prepareCompiled('indexed',[[0,key]]),guarded=m.prepareCompiled('guarded',[[0,key]]);
const checked=m.prepareCompiled('checked',[[0,key]]),yes=codecs.checked.params[2].capture(true),no=codecs.checked.params[2].capture(false);
const edited=m.prepareCompiled('edited',[[0,key]]);
const result=(name,token,args)=>materialize(name,m.callPreparedCompiled(token,args));
const NativeMap=globalThis.Map;
let builds=0;globalThis.Map=new Proxy(NativeMap,{construct(target,args){builds++;return Reflect.construct(target,args,target);}});
try{
 assert.deepEqual(result('indexed',token,[one]),[false,true,true,false]);assert.equal(builds,1);
 const changed=result('indexed',token,[two]);assert.deepEqual(changed,[false,true,false]);changed[0]=true;
 assert.deepEqual(result('indexed',token,[two]),[false,true,false]);assert.equal(builds,1);
 // Branch-local construction remains lazy, then persists across varying calls.
 assert.deepEqual(result('guarded',guarded,[empty]),[]);assert.equal(builds,1);
 assert.deepEqual(result('guarded',guarded,[one]),[false,true,true,false]);assert.equal(builds,2);
 assert.deepEqual(result('guarded',guarded,[empty]),[]);assert.equal(builds,2);
 assert.deepEqual(result('guarded',guarded,[two]),[false,true,false]);assert.equal(builds,2);
 // Declared requires runs before any retained computation, on every invocation.
 assert.throws(()=>result('checked',checked,[one,no]),/precondition/);assert.equal(builds,2);
 assert.deepEqual(result('checked',checked,[one,yes]),[false,true,true,false]);assert.equal(builds,3);
 assert.throws(()=>result('checked',checked,[one,no]),/precondition/);assert.equal(builds,3);
 assert.deepEqual(result('checked',checked,[two,yes]),[false,true,false]);assert.equal(builds,3);
 // A consumed owner cannot be reused as readonly merely because its original
 // initializer has fixed inputs. Dynamic mutation must rebuild that owner.
 assert.deepEqual(result('edited',edited,[two]),[true,true,false]);assert.equal(builds,4);
 assert.deepEqual(result('edited',edited,[one]),[true,true,true,false]);assert.equal(builds,5);
}finally{globalThis.Map=NativeMap;}
// Throwing computations are retried, never retained as successful values.
const retry=m.prepareCompiled('indexed',[[0,key]]),sentinel=new Error('construction failed');let attempts=0;
globalThis.Map=new Proxy(NativeMap,{construct(target,args){if(++attempts===1)throw sentinel;return Reflect.construct(target,args,target);}});
try{assert.throws(()=>result('indexed',retry,[one]),e=>e===sentinel);assert.deepEqual(result('indexed',retry,[one]),[false,true,true,false]);assert.deepEqual(result('indexed',retry,[two]),[false,true,false]);assert.equal(attempts,2);}finally{globalThis.Map=NativeMap;}
// Immutable cached lists stay shared through every residual cut. The ordinary
// final-use concat optimization must never append into a retained list.
for(const [name,argsA,argsB]of [['transformed',[[1,2,0xffffffff],3],[[1,2,0xffffffff],7]],['shadowed',[[1,2,3],[8,9]],[[1,2,3],[17]]]]){
 const c=codecs[name],fixed=c.params[0].capture(argsA[0]),t=m.prepareCompiled(name,[[0,fixed]]);
 for(let i=0;i<20;i++)for(const args of [argsA,argsB]){
  const full=[fixed,c.params[1].capture(args[1])];assert.deepEqual(materialize(name,m.callPreparedCompiled(t,[full[1]])),materialize(name,m.callCompiled(name,full)));
 }
}
assert.throws(()=>m.prepareCompiled('missing',[]),/unsupported/);
for(const bindings of [[[0,key],[0,key]],[[-1,key]],[[2,key]],[[0.5,key]],[[0,{}]],[[0,yes]]])assert.throws(()=>m.prepareCompiled('indexed',bindings));
assert.throws(()=>m.callPreparedCompiled({},[one]),/invalid prepared/);assert.throws(()=>m.callPreparedCompiled(token,[]),/invalid prepared/);assert.throws(()=>m.callPreparedCompiled(token,[{}]),/owner mismatch/);
const foreign=await import('./program.mjs?foreign-owner');assert.throws(()=>foreign.callPreparedCompiled(token,[one]),/invalid prepared/);assert.throws(()=>foreign.prepareCompiled('indexed',[[0,key]]),/owner mismatch/);
console.log(JSON.stringify({calls,referenceCases:cases.length,readonlyIndexBuilds:1,guardedBuilds:1,consumedOwnersRebuilt:true,failedConstructionRetried:true}));
"#).unwrap();
 let output=Command::new("node").arg(root.join("test.mjs")).output().unwrap();
 assert!(output.status.success(),"{}",String::from_utf8_lossy(&output.stderr));println!("{}",String::from_utf8_lossy(&output.stdout));fs::remove_dir_all(root).unwrap();
}
