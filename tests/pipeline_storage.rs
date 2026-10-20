use ink_core::{core::CheckedModule, eval, syntax::parse};
use serde_json::{json,Value};
fn wire(v:&eval::Value)->Value{match v{eval::Value::F32(n)=>json!({"F32Bits":n}),eval::Value::F64(n)=>json!({"F64Bits":n.to_string()}),eval::Value::List(xs)=>xs.iter().map(wire).collect(),eval::Value::Record(_,fs)=>fs.iter().map(|(n,v)|(n.clone(),wire(v))).collect(),_=>v.json()}}
use std::{fs, process::Command};

#[test]
fn pipeline_storage_matches_reference_and_keeps_aliases_caches_and_stage_order() {
    let dir=std::env::temp_dir().join(format!("ink-pipeline-storage-{}",std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    for mode in ["native","captured","bits","wide"] {
        let mut source=include_str!("fixtures/pipeline_storage.ink").to_owned();
        if mode=="wide" {source=source.replace("f32","f64").replace("0.5","0.5f64").replace("0.0","0.0f64");}
        if matches!(mode,"captured"|"bits") {source+="state Counter:u32=0;";}
        if mode=="bits" {source+="fn owner(x:Buffer<f32>)->Buffer<f32>{return x;}";}
        let checked=CheckedModule::from_source(parse(&source).unwrap()).unwrap();
        let words=if mode=="wide" {vec![0u64,1,1<<63,0x3ff0000000000001,0x7ff0000000000000,0x7ff0000000000001,0xfff8000000001234]}
            else {vec![0,1,0x80000000,0x3f800001,0x7f800000,0x7f800001,0xffc12345]};
        let scalar=|b:u64|if mode=="wide" {json!({"F64Bits":b.to_string()})}else{json!({"F32Bits":b})};
        let mut cases=vec![];
        for n in 0..=words.len() {
            let xs=words.iter().take(n).copied().map(scalar).collect::<Vec<_>>();
            let amount=scalar(words[n%words.len()]);
            let rows=xs.iter().map(|x|json!({"items":xs,"amount":x})).collect::<Vec<_>>();
            for (name,args) in [
                ("stages",json!([xs,amount])),("wrapped",json!([xs,amount])),
                ("helpers",json!([xs,amount])),("helper_alias",json!([xs,amount])),("helper_pending",json!([xs,amount])),("borrowed",json!([xs,amount])),("shared",json!([xs,amount])),
                ("captured",json!([xs,amount])),("pending",json!([xs,amount])),
                ("selected",json!([xs,amount,true])),("selected",json!([xs,amount,false])),
                ("indexed",json!([[0,2,7],xs,amount])),("cached_rows",json!([[0,2,7],rows]))] {
                let f=checked.program().functions.iter().find(|f|f.name==name).unwrap();
                let inputs=args.as_array().unwrap().iter().zip(&f.params).map(|(v,(_,t))|eval::Value::from_program_json(v,t,checked.program()).unwrap()).collect();
                let expected=wire(&eval::call_checked(&checked,name,inputs,&mut 1_000_000).unwrap());
                cases.push(json!({"name":name,"args":args,"expected":expected}));
            }
        }
        let emitted=ink_lowering_js::lower(&checked).unwrap();
        // Observe only generated list allocations, including literal residual lists.
        let source=emitted.javascript.replace("=[];","=new Array(0);").replace("===undefined?[]:","===undefined?new Array(0):");
        fs::write(dir.join("program.mjs"),source).unwrap();
        fs::write(dir.join("cases.json"),serde_json::to_vec(&cases).unwrap()).unwrap();
        fs::write(dir.join("test.mjs"),r#"
import assert from 'node:assert/strict';import fs from 'node:fs';import * as m from './program.mjs';
let comparisons=0;
for(const row of JSON.parse(fs.readFileSync(new URL('./cases.json',import.meta.url)))){
 const c=m.compiledValues[row.name],handles=row.args.map((v,i)=>c.params[i].capture(v));
 const before=structuredClone(row.args),expected=row.expected;
 assert.deepEqual(c.result.materialize(m.callCompiled(row.name,handles)),expected);comparisons++;
 for(const bound of [[],[0],handles.map((_,i)=>i)]){
  const token=m.prepareCompiled(row.name,bound.map(i=>[i,handles[i]])),args=handles.filter((_,i)=>!bound.includes(i));
  for(let repeat=0;repeat<3;repeat++){
   assert.deepEqual(c.result.materialize(m.callPreparedCompiled(token,args)),expected);comparisons++;
   if(Array.isArray(expected)){
    const out=new Array(expected.length+2).fill('tail');const count=m.callPreparedCompiledInto(token,args,out);
    assert.equal(count,expected.length);assert.deepEqual(out.slice(count),['tail','tail']);
    assert.deepEqual(c.result.materialize(c.result.capture(out.slice(0,count))),expected);comparisons++;
    for(const v of out){if(v&&typeof v==='object'){if(v.items)v.items.push(99);if(Object.hasOwn(v,'amount'))v.amount=88;}}
   }
  }
 }
 assert.deepEqual(row.args,before);
}
const NativeArray=globalThis.Array;let allocations=0;
globalThis.Array=new Proxy(NativeArray,{construct(t,args){allocations++;return Reflect.construct(t,args);}});
const xs=[-1,2,3,4],out=new Float32Array(6).fill(77);
for(const name of ['stages','wrapped','helpers']){const c=m.compiledValues[name],h=[c.params[0].capture(xs),c.params[1].capture(.25)];m.callCompiled(name,h);allocations=0;m.callCompiled(name,h);assert.equal(allocations,name==='wrapped'?2:1,name);assert.deepEqual(xs,[-1,2,3,4]);}
const c=m.compiledValues.indexed,h=[c.params[0].capture([0,2]),c.params[1].capture(xs),c.params[2].capture(.25)],token=m.prepareCompiled('indexed',[[0,h[0]]]);
m.callPreparedCompiled(token,h.slice(1));allocations=0;assert.equal(m.callPreparedCompiledInto(token,h.slice(1),out),4);assert.equal(allocations,0,'residual result list eliminated');assert.deepEqual(Array.from(out),[-.75,2,3.25,4,77,77]);
for(let i=0;i<4;i++){allocations=0;assert.equal(m.callPreparedCompiledInto(token,h.slice(1),out),4);assert.equal(allocations,0);assert.deepEqual(c.result.materialize(m.callPreparedCompiled(token,h.slice(1))),c.result.materialize(c.result.capture(xs.map((x,j)=>j%2===0?x+.25:x))));}
globalThis.Array=NativeArray;
let order=[];
const input=[1,2,3];input.forEach((x,i)=>Object.defineProperty(input,i,{get(){order.push('read'+i);return x;}}));
const destination=new Proxy([],{set(t,k,v){order.push('write'+k);t[k]=v;return true;}});
assert.equal(m.functionsInto.order(input,destination),3);assert.deepEqual(order,['read0','read1','read2','write0','write1','write2']);
console.log(`${comparisons} complete compiled/prepared/destination reference comparisons; allocation, alias, cache, detachment, tail and eager-stage order checks passed`);
"#).unwrap();
        let out=Command::new("node").arg(dir.join("test.mjs")).output().unwrap();
        assert!(out.status.success(),"{mode}: {}{}",String::from_utf8_lossy(&out.stdout),String::from_utf8_lossy(&out.stderr));
        println!("{mode}: {}",String::from_utf8_lossy(&out.stdout));
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn fresh_outer_storage_keeps_typed_array_layout_and_element_conversion_separate() {
 let dir=std::env::temp_dir().join(format!("ink-pipeline-layout-{}",std::process::id()));fs::create_dir_all(&dir).unwrap();
 let base="module slices;record Row{value:f32,}fn halve(xs:List<u32>)->List<f32>{return xs.take(3).map(fn(x)=>f32_from_u32(x)*0.5);}fn keep(xs:List<u32>)->List<u32>{let copied=xs.drop(1);return copied.filter(fn(x)=>x>2);}fn cumulative(xs:List<u32>)->List<u32>{return xs.take(3).scan();}fn flags(xs:List<u64>)->List<Bool>{return xs.drop(0).map(fn(x)=>x>1);}fn rows(xs:List<u32>)->List<Row>{return xs.take(3).map(fn(x)=>Row{value:f32_from_u32(x)*0.5});}";
 for mode in ["native","captured","bits","wide"] {
  let mut source=base.to_owned();
  if mode=="wide" {source=source.replace("f32","f64").replace("0.5","0.5f64");}
  if matches!(mode,"captured"|"bits") {source+="state Counter:u32=0;";}
  if mode=="bits" {source+="fn owner(x:Buffer<f32>)->Buffer<f32>{return x;}";}
  let checked=CheckedModule::from_source(parse(&source).unwrap()).unwrap();
  fs::write(dir.join("program.mjs"),ink_lowering_js::lower(&checked).unwrap().javascript).unwrap();
  fs::write(dir.join("test.mjs"),r#"
import assert from'node:assert/strict';import{functions as f}from'./program.mjs';
for(const values of [[],[1],[1,2,3,4]]){
 for(const input of [values.slice(),new Uint32Array(values),new Uint32Array([99,...values,88]).subarray(1,values.length+1)]){
  const before=Array.from(input),half=values.slice(0,3).map(x=>x*.5);
  assert.deepEqual(f.halve(input),half);assert.deepEqual(f.keep(input),values.slice(1).filter(x=>x>2));
  let total=0;assert.deepEqual(f.cumulative(input),values.slice(0,3).map(x=>total=(total+x)>>>0));
  assert.deepEqual(f.rows(input),half.map(value=>({value})));assert.deepEqual(Array.from(input),before);
 }
 const wide=new BigUint64Array(values.map(BigInt));assert.deepEqual(f.flags(wide),values.map(x=>x>1));assert.deepEqual(Array.from(wide),values.map(BigInt));
}
console.log('39 typed-array slice/map/filter/scan/record cases preserve storage kinds, element conversions and borrowed inputs');
"#).unwrap();
  let out=Command::new("node").arg(dir.join("test.mjs")).output().unwrap();assert!(out.status.success(),"{mode}: {}",String::from_utf8_lossy(&out.stderr));println!("{mode}: {}",String::from_utf8_lossy(&out.stdout));
 }
 fs::remove_dir_all(dir).unwrap();
}

#[test]
fn large_residual_output_loops_keep_bounded_cached_result_fallback() {
 fn sum(xs:&[&str])->String{if xs.len()==1{xs[0].into()}else{let n=xs.len()/2;format!("({}+{})",sum(&xs[..n]),sum(&xs[n..]))}}
 let leaves=448;let expression=sum(&vec!["x";leaves]);
 let source=format!("module bounded_output;fn wide(keys:List<u32>,xs:List<f32>)->List<f32>{{let table:Map<u32,Bool>=foldr(keys,Map.empty(),fn(k)=>fn(m)=>m.set(k,true));return xs.map_indexed(fn(i)=>fn(x)=>choose(table.contains(i),{expression},{expression}));}}");
 let checked=CheckedModule::from_source(parse(&source).unwrap()).unwrap();let emitted=ink_lowering_js::lower(&checked).unwrap();
 let factory=emitted.javascript.split("function __prepareFn0(").nth(1).unwrap().split("const __preparedFactories").next().unwrap();
 assert_eq!(factory.matches("Math.fround(").count(),2*(leaves-1),"large output loop must use one bounded result/copy body");
 let dir=std::env::temp_dir().join(format!("ink-bounded-output-{}",std::process::id()));fs::create_dir_all(&dir).unwrap();
 let mut cases=vec![];
 for bits in [0u32,0x80000000,1,0x3dcccccd,0x7f800001,0x7f7fffff]{
  let args=vec![eval::Value::List(vec![eval::Value::U32(0)]),eval::Value::List(vec![eval::Value::F32(bits)])];
  let expected=wire(&eval::call_checked(&checked,"wide",args,&mut 1_000_000).unwrap());cases.push(json!({"bits":bits,"expected":expected}));
 }
 fs::write(dir.join("program.mjs"),emitted.javascript.replace("=[];","=new Array(0);")).unwrap();fs::write(dir.join("cases.json"),serde_json::to_vec(&cases).unwrap()).unwrap();
 fs::write(dir.join("test.mjs"),r#"
import assert from'node:assert/strict';import fs from'node:fs';import * as m from'./program.mjs';
assert.equal(m.metadata.prepared_compiled.functions.find(f=>f.function==='wide').factory,true);
const c=m.compiledValues.wide,keys=c.params[0].capture([0]),token=m.prepareCompiled('wide',[[0,keys]]),NativeArray=globalThis.Array;let allocations=0;globalThis.Array=new Proxy(NativeArray,{construct(t,args){allocations++;return Reflect.construct(t,args);}});
for(const row of JSON.parse(fs.readFileSync(new URL('./cases.json',import.meta.url)))){
 const xs=c.params[1].capture([{F32Bits:row.bits}]);assert.deepEqual(c.result.materialize(m.callPreparedCompiled(token,[xs])),row.expected);
 const out=['tail','tail'];allocations=0;assert.equal(m.callPreparedCompiledInto(token,[xs],out),1);assert.equal(allocations,1,'bounded fallback retains its result array');assert.equal(out[1],'tail');assert.deepEqual(c.result.materialize(c.result.capture(out.slice(0,1))),row.expected);
}
globalThis.Array=NativeArray;console.log('bounded residual fallback retains cached preparation, 894 source rounding sites, exact exceptional floats and destination tails');
"#).unwrap();
 let out=Command::new("node").arg(dir.join("test.mjs")).output().unwrap();assert!(out.status.success(),"{}",String::from_utf8_lossy(&out.stderr));println!("{}",String::from_utf8_lossy(&out.stdout));fs::remove_dir_all(dir).unwrap();
}
