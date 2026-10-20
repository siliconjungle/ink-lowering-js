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
