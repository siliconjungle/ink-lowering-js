use ink_core::{core::CheckedModule,syntax::parse};
use std::{fs,process::Command};

#[test]
fn dense_f32_owners_preserve_tags_ranges_growth_and_map_order(){
 use ink_core::eval;
 use serde_json::json;
 let source=r#"module DenseOwners;
fn mapped(x:f32,y:f32,n:u32)->List<f32>{return buffer_fill(n,x).map_indexed(fn(i)=>fn(v)=>choose(i==1,y,v)).freeze().concat([x,y]);}
fn grown(x:f32,y:f32,n:u32)->List<f32>{return buffer_fill(n,x).set(2,y).fill_range(3,2,y).copy_within(1,0,4).swap(0,6).pop().truncate(4).push(x).freeze().concat([y]);}
fn parts(x:f32,y:f32,n:u32)->List<f32>{return buffer_fill(n,x).split_at(3,fn(left)=>fn(right)=>left.set(1,y).map(fn(v)=>v).append(right.map_indexed(fn(i)=>fn(v)=>v+x))).freeze();}
fn ranges(x:f32,y:f32,n:u32)->List<f32>{return buffer_fill(n,x).split_at(1,fn(left)=>fn(right)=>right.truncate(5)).map_indexed(fn(i)=>fn(v)=>choose(i==2,y,v)).push(x).freeze();}
fn observed(x:f32,y:f32,n:u32)->List<f32>{let b=buffer_fill(n,x);let first=b.borrow_range(1,3,fn(v)=>v.at_or(0,y));return [first].concat(b.set(1,y).freeze());}
fn frozen(x:f32,y:f32,n:u32)->List<f32>{return buffer_fill(n,x).freeze().concat([y]);}
"#;
 let checked=CheckedModule::from_source(parse(source).unwrap()).unwrap();
 let mut code=ink_lowering_js::lower(&checked).unwrap().javascript;
 code.push_str(r#"
export function denseOwnerProbe(){
 const b=__bufferFillF32(8,0),dense=b.data instanceof Float32Array;
 const [a,c]=__bufferSplit(b,3);a.set(1,__nfFromBits(0x7fc12345));
 return {dense,promoted:Array.isArray(a.data),siblingDense:c.data instanceof Float32Array,
    tag:a.item(1).F32Bits,sibling:c.freeze(),growable:Array.isArray(a.freeze())};
}
"#);
 let bits=[0u32,0x80000000,1,0x80000001,0x007fffff,0x00800000,0x3f800001,0x7f7fffff,0x7f800000,0xff800000,0x7f812345,0xffc12345];
 let mut cases=vec![];
 for (i,&x) in bits.iter().enumerate(){for n in [0,1,7,65]{
  let args=json!([{"F32Bits":x},{"F32Bits":bits[(i+5)%bits.len()]},n]);
  for name in ["mapped","grown","parts","ranges","observed","frozen"]{
   let f=checked.program().functions.iter().find(|f|f.name==name).unwrap();
   let values=args.as_array().unwrap().iter().zip(&f.params).map(|(v,(_,t))|eval::Value::from_program_json(v,t,checked.program()).unwrap()).collect();
   let expected=eval::call_checked(&checked,name,values,&mut 1_000_000).unwrap().json();
   cases.push(json!({"name":name,"args":args,"expected":expected}));
  }
 }}
 let dir=std::env::temp_dir().join(format!("ink-dense-owners-{}",std::process::id()));fs::create_dir_all(&dir).unwrap();
 fs::write(dir.join("program.mjs"),code).unwrap();fs::write(dir.join("cases.json"),serde_json::to_vec(&cases).unwrap()).unwrap();
 fs::write(dir.join("test.mjs"),r#"
import assert from 'node:assert/strict';import fs from 'node:fs';import * as m from './program.mjs';
assert.deepEqual(m.denseOwnerProbe(),{dense:true,promoted:true,siblingDense:true,tag:0x7fc12345,sibling:[0,0,0,0,0],growable:true});
for(const row of JSON.parse(fs.readFileSync(new URL('cases.json',import.meta.url)))){
 const codec=m.valueCodecs[row.name],expected=codec.result.encode(row.expected),entry=m.compiledValues[row.name];
 assert.deepEqual(codec.result.encode(m.functions[row.name](...row.args)),expected,row.name);
 const args=row.args.map((x,i)=>entry.params[i].capture(x));
 assert.deepEqual(entry.result.encode(m.callCompiled(row.name,args)),expected);
 const token=m.prepareCompiled(row.name,[[0,args[0]]]);
 for(let k=0;k<2;k++){
  const result=m.callPreparedCompiled(token,args.slice(1));assert.deepEqual(entry.result.encode(result),expected);
  const destination=new Array(row.expected.length+2).fill('tail');
  assert.equal(m.callPreparedCompiledInto(token,args.slice(1),destination),row.expected.length);
  assert.deepEqual(codec.result.encode(destination.slice(0,row.expected.length)),expected);
  assert.deepEqual(destination.slice(row.expected.length),['tail','tail']);
 }
}
"#).unwrap();
 let out=Command::new("node").arg(dir.join("test.mjs")).output().unwrap();fs::remove_dir_all(dir).unwrap();
 assert!(out.status.success(),"{}",String::from_utf8_lossy(&out.stderr));
}

#[test]
fn dense_private_f32_captures_preserve_bits_growth_aliasing_and_limits(){
 let source=r#"module DenseFloat;
fn copy(xs:List<f32>)->List<f32>{return xs;}
fn grow(xs:List<f32>)->List<f32>{return xs.drop(1).take(3).concat([7.0]);}
fn owned(xs:List<f32>)->List<f32>{let b:Buffer<f32>=buffer(xs);return b.push(9.0).freeze();}
fn equal(xs:List<f32>,ys:List<f32>)->Bool{return xs==ys;}
fn leaf(xs:List<f32>,index:u32)->f32{return xs.at_or(index,0.0);}
fn kernel(xs:List<f32>,ids:List<u32>)->f32{return foldl(ids,0.0,fn(i)=>fn(acc)=>acc+leaf(xs,i));}
"#;
 let module=CheckedModule::from_source(parse(source).unwrap()).unwrap();
 let mut code=ink_lowering_js::lower(&module).unwrap().javascript;
 // Test-only storage observation; production handles never expose payloads.
 code.push_str("\nexport const dense=h=>__CompiledHandle.entry(h).value instanceof Float32Array;\n");
 let root=std::env::temp_dir().join(format!("ink-dense-float-{}",std::process::id()));fs::create_dir_all(&root).unwrap();
 fs::write(root.join("program.mjs"),code).unwrap();
 fs::write(root.join("test.mjs"),r#"
import assert from 'node:assert/strict';import * as m from './program.mjs';
const cases=[[],[0,-0,1,3.25],[{F32Bits:1},{F32Bits:0x80000001},{F32Bits:0x7f800000}],
 [{F32Bits:0x7fc12345},-0,{F32Bits:0xff812345}],[-0,{F32Bits:0x7f812345},4],
 [1e20,-1e20,3],Array.from({length:257},(_,i)=>Math.fround((i-128)/7)),
 Array.from({length:70},(_,i)=>i===40?{F32Bits:0x7fc12345}:Math.fround(i/3)),
 Array.from({length:1024},(_,i)=>Math.fround((i-512)/9)),
 Array.from({length:1030},(_,i)=>i===900?{F32Bits:0x7fc12345}:Math.fround(i/5))];
for(const xs of cases){
 const codec=m.compiledValues.copy.params[0],before=codec.capture(xs);
 const hasNaN=xs.some(v=>v&&typeof v==='object'&&(v.F32Bits&0x7fffffff)>0x7f800000);
 // Lists shorter than 1024 stay slot arrays; longer NaN-free lists are dense.
 assert.equal(m.dense(before),!hasNaN&&xs.length>=1024);
 for(const name of ['copy','grow','owned']){
  const c=m.compiledValues[name],plain=m.functions[name](xs),expected=m.valueCodecs[name].result.decode(m.valueCodecs[name].result.encode(plain)),result=m.callCompiled(name,[before]);
  const actual=c.result.materialize(result);assert.deepEqual(actual,expected,name);assert.equal(Array.isArray(actual),true);
  assert.deepEqual(c.result.encode(result),m.valueCodecs[name].result.encode(expected));
  const decoded=codec.decode(codec.encode(before));assert.deepEqual(codec.materialize(decoded),codec.materialize(before));
  const out=new Array(expected.length+2).fill('tail');const prepared=m.prepareCompiled(name,[[0,before]]);
  assert.equal(m.callPreparedCompiledInto(prepared,[],out),expected.length);
  assert.deepEqual(out.slice(0,expected.length),plain);assert.deepEqual(out.slice(expected.length),['tail','tail']);
  actual.fill(99);assert.deepEqual(c.result.materialize(result),expected);
 }
 const eq=m.compiledValues.equal;assert.equal(eq.result.materialize(m.callCompiled('equal',[before,codec.decode(codec.encode(before))])),true);
 const kernel=m.compiledValues.kernel,ids=[0,1,2,0,1000],captured=kernel.params[1].capture(ids);
 const expected=m.valueCodecs.kernel.result.decode(m.valueCodecs.kernel.result.encode(m.functions.kernel(xs,ids)));
 assert.deepEqual(kernel.result.materialize(m.callCompiled('kernel',[before,captured])),expected);
}
const codec=m.compiledValues.copy.params[0],host=[1,-0,2],captured=codec.capture(host);host[0]=99;
assert.deepEqual(codec.materialize(captured),[{F32Bits:0x3f800000},{F32Bits:0x80000000},{F32Bits:0x40000000}]);
// Ordered getter reads and errors precede any dense conversion.
let reads=[];const xs=[1,2,3];xs.forEach((v,i)=>Object.defineProperty(xs,i,{get(){reads.push(i);if(i===1)throw Error('sentinel');return v;}}));
assert.throws(()=>codec.capture(xs),/sentinel/);assert.deepEqual(reads,[0,1]);
for(const value of [[1,2],new Float32Array([1,2])])for(const nodes of [0,1,2,3,4]){
 const accepts=fn=>{try{fn();return true;}catch{return false;}};
 assert.equal(accepts(()=>codec.capture(value,{nodes})),accepts(()=>new m.ValueCodec(codec.descriptor).encode(value,{nodes})));
}
"#).unwrap();
 let result=Command::new("node").arg(root.join("test.mjs")).output().unwrap();
 fs::remove_dir_all(root).unwrap();assert!(result.status.success(),"{}",String::from_utf8_lossy(&result.stderr));
}

#[test]
fn immutable_handles_reuse_checked_type_facts_without_erasing_other_requires(){
 let source=r#"module Guarantees;
refined record Positive{value:u32,} invariant value.value>0;
fn get(value:Positive)->u32{return value.value;}
fn copy(value:Positive)->Positive{return value;}
spec fn guarded(value:Positive,allowed:Bool)->u32 requires allowed;ensures result==value.value;
implementation selected for guarded{return value.value;}
"#;
 let module=CheckedModule::from_source(parse(source).unwrap()).unwrap();
 let mut code=ink_lowering_js::lower(&module).unwrap().javascript;
 // Instrument actual guard invocations without changing their return values.
 code.push_str(r#"
const guardCalls=new Map();for(const index of new Set([...__compiledEntries.values()].map(e=>e.precondition).filter(i=>i!==null))){const original=__implementations[index];__implementations[index]=(...values)=>{guardCalls.set(index,(guardCalls.get(index)||0)+1);return original(...values);};}
export const countGuards=name=>guardCalls.get(__compiledEntries.get(name).precondition)||0;
"#);
 let root=std::env::temp_dir().join(format!("ink-compiled-guarantees-{}",std::process::id()));fs::create_dir_all(&root).unwrap();fs::write(root.join("program.mjs"),code).unwrap();
 fs::write(root.join("test.mjs"),r#"
import assert from 'node:assert/strict';import * as m from './program.mjs';
const codec=m.compiledValues.get.params[0],host={value:7},captured=codec.capture(host);host.value=0;
const get=h=>m.compiledValues.get.result.materialize(m.callCompiled('get',[h]));
assert.equal(get(captured),7);assert.equal(m.countGuards('get'),1);
for(let i=0;i<10;i++)assert.equal(get(captured),7);assert.equal(m.countGuards('get'),1);
const copied=m.callCompiled('copy',[captured]);assert.equal(m.countGuards('copy'),0);
const materialized=codec.materialize(copied);materialized.value=0;
assert.equal(get(copied),7);assert.equal(m.countGuards('get'),1);
const decoded=codec.decode(codec.encode(captured));assert.equal(get(decoded),7);assert.equal(m.countGuards('get'),2);
const invalid=codec.capture({value:0});for(let i=0;i<2;i++)assert.throws(()=>get(invalid),/precondition/);assert.equal(m.countGuards('get'),4);
const g=m.compiledValues.guarded,yes=g.params[1].capture(true),no=g.params[1].capture(false);
for(let i=0;i<2;i++){assert.throws(()=>m.callCompiled('guarded',[copied,no]),/precondition/);assert.equal(g.result.materialize(m.callCompiled('guarded',[copied,yes])),7);}assert.equal(m.countGuards('guarded'),4);
assert.throws(()=>get({}),/owner mismatch/);assert.throws(()=>get(g.params[1].capture(true)),/type or owner mismatch/);
"#).unwrap();
 let result=Command::new("node").arg(root.join("test.mjs")).output().unwrap();fs::remove_dir_all(root).unwrap();assert!(result.status.success(),"{}",String::from_utf8_lossy(&result.stderr));
}
