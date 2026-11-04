use ink_core::{core::CheckedModule,syntax::parse};
use std::{fs,process::Command};

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
 code.push_str("\nexport const dense=h=>__compiledHandles.get(h).value instanceof Float32Array;\n");
 let root=std::env::temp_dir().join(format!("ink-dense-float-{}",std::process::id()));fs::create_dir_all(&root).unwrap();
 fs::write(root.join("program.mjs"),code).unwrap();
 fs::write(root.join("test.mjs"),r#"
import assert from 'node:assert/strict';import * as m from './program.mjs';
const cases=[[],[0,-0,1,3.25],[{F32Bits:1},{F32Bits:0x80000001},{F32Bits:0x7f800000}],
 [{F32Bits:0x7fc12345},-0,{F32Bits:0xff812345}],[-0,{F32Bits:0x7f812345},4],
 [1e20,-1e20,3],Array.from({length:257},(_,i)=>Math.fround((i-128)/7)),
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
