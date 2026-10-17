use ink_core::{core::CheckedModule,eval,syntax::parse};
use serde_json::{json,Value};
use std::{fs,process::Command};

#[test]
fn direct_owned_helpers_expand_without_borrowed_list_ownership_or_call_reordering(){
 let mut source=r#"module DirectOwners;
record Out{values:List<f32>,shared:List<f32>,indexed:List<f32>,}
fn add(b:Buffer<f32>,i:u32,x:f32)->Buffer<f32>{let prior=b.at_or(i,0.0);return b.set(i,prior+x);}
fn stage(b:Buffer<f32>,xs:List<f32>,x:f32)->Buffer<f32>{let changed=xs.map(fn(a)=>a+x);return b.append(buffer(changed));}
fn index(m:Map<u32,f32>,i:u32,x:f32)->Map<u32,f32>{let prior=m.get_or(i,-0.0);return m.set(i,prior+x);}
fn walk(xs:List<f32>,b:Buffer<f32>)->Buffer<f32>{if length(xs)==0{return b;}return walk(xs.drop(1),b.push(xs.at_or(0,0.0)));}
fn run(xs:List<f32>,x:f32,n:u32)->Out{
 let b=repeat_until(n,buffer(xs),fn(b)=>false,fn(i)=>fn(b)=>add(b,rem_or(i,3,0),x));
 let b=b.split_at(1,fn(a)=>fn(b)=>stage(a,xs,x).append(stage(b,xs,-x)));
 let m:Map<u32,f32>=Map.empty();let m=repeat_until(n,m,fn(m)=>false,fn(i)=>fn(m)=>index(m,rem_or(i,3,0),x));
 return Out{values:walk(xs,b).freeze(),shared:xs,indexed:m.values()};
}
fn parallel(xs:List<f32>,x:f32,n:u32)->List<Out>{let keys:List<u32>=[0,1,2];return parallel_map(keys,1,fn(i)=>fn(k)=>run(xs,x,n));}
"#.to_owned();
 source+=&format!("fn wide(b:Buffer<f32>)->Buffer<f32>{{let xs:List<f32>=[{}];return b.push(xs.at_or(9,0.0));}}fn fallback(xs:List<f32>,x:f32,n:u32)->Out{{return Out{{values:wide(buffer(xs)).freeze(),shared:xs,indexed:[]}};}}",vec!["0.0";530].join(","));
 let directory=std::env::temp_dir().join(format!("ink-owned-parameters-{}",std::process::id()));fs::create_dir_all(&directory).unwrap();
 let bits=[0u32,0x80000000,1,0x80000001,0x007fffff,0x00800000,0x3f800001,0xbf800002,0x7f7fffff,0xff7fffff,0x7f800000,0xff800000,0x7f800001,0x7fc12345,0xffc12345,0x4b800000];
 for mode in ["native","bits"]{
  let source=if mode=="bits"{format!("{source}\nstate Rows:Table<u32,List<f32>>=Table.empty();")}else{source.clone()};
  let checked=CheckedModule::from_source(parse(&source).unwrap()).unwrap();let mut cases=vec![];
  let mut add=|name:&str,args:Value|{let f=checked.program().functions.iter().find(|f|f.name==name).unwrap();let values=args.as_array().unwrap().iter().zip(&f.params).map(|(v,(_,t))|eval::Value::from_program_json(v,t,checked.program()).unwrap()).collect();let expected=eval::call_checked(&checked,name,values,&mut 1_000_000).unwrap().json();cases.push(json!({"name":name,"args":args,"expected":expected}));};
  for(i,b)in bits.iter().enumerate(){let x=json!({"F32Bits":b});let y=json!({"F32Bits":bits[(i+3)%bits.len()]});
   for n in [0,1,4]{for xs in [json!([]),json!([x,y]),json!([y,x,0.5,-0.0,3.0])]{for name in ["run","fallback"]{add(name,json!([xs,x,n]));}}}
   if i<3{add("parallel",json!([[x,y],x,2]));}
  }
  let emitted=ink_lowering_js::lower(&checked).unwrap();fs::write(directory.join("program.mjs"),&emitted.javascript).unwrap();
  let mut instrumented=emitted.javascript;
  for name in ["add","stage","index","walk","wide"]{
   let i=checked.program().functions.iter().position(|f|f.name==name).unwrap();let start=instrumented.find(&format!("function __fn{i}(" )).unwrap();let body=start+instrumented[start..].find("{\n").unwrap()+2;
   instrumented.insert_str(body,&format!("globalThis.__owned_{name}=(globalThis.__owned_{name}??0)+1;\n"));
  }
  fs::write(directory.join("instrumented.mjs"),instrumented).unwrap();fs::write(directory.join("cases.json"),serde_json::to_vec(&cases).unwrap()).unwrap();
  fs::write(directory.join("test.mjs"),r#"
import assert from 'node:assert/strict';import fs from 'node:fs';import * as m from './program.mjs';
const cases=JSON.parse(fs.readFileSync(new URL('cases.json',import.meta.url)));
for(const c of cases){const before=structuredClone(c.args);assert.deepEqual(m.call(c.name,c.args),c.expected,c.name);assert.deepEqual(c.args,before,'caller list stays shared');
 const entry=m.compiledValues[c.name],args=c.args.map((v,i)=>entry.params[i].capture(v)),expected=m.valueCodecs[c.name].result.encode(c.expected);
 assert.deepEqual(entry.result.encode(m.callCompiled(c.name,args)),expected);
 if(c.name!=='parallel'){const token=m.prepareCompiled(c.name,[[0,args[0]]]);for(let i=0;i<2;i++){
  const actual=m.callPreparedCompiled(token,args.slice(1));assert.deepEqual(entry.result.encode(actual),expected);const host=entry.result.materialize(actual);host.values.splice(0);host.shared.splice(0);host.indexed.splice(0);
 }}
}
const q=await import('./instrumented.mjs');for(const n of ['add','stage','index','walk','wide'])globalThis['__owned_'+n]=0;
assert.deepEqual(q.call('run',[[1.5,-0],0.5,3]),m.call('run',[[1.5,-0],0.5,3]));
for(const n of ['add','stage','index'])assert.equal(globalThis['__owned_'+n],0,n+' expanded');assert.ok(globalThis.__owned_walk>0,'recursive helper remains ordinary');
assert.deepEqual(q.call('fallback',[[1.5,-0],0.5,3]),m.call('fallback',[[1.5,-0],0.5,3]));assert.equal(globalThis.__owned_wide,1,'oversized helper remains ordinary');
for(const n of ['add','stage','index','walk','wide']){assert.equal(m.functions[n],undefined);assert.throws(()=>m.call(n,[]),/temporary storage/);delete globalThis['__owned_'+n];}
assert.ok(m.parallelStatistics().workers>=9);assert.ok(m.parallelStatistics().lastWorkerIds.length>=2);
console.log(JSON.stringify({status:'passed',cases:cases.length,expanded:true,ordinaryRecursiveAndBudgetFallback:true,workers:m.parallelStatistics().workers}));
"#).unwrap();
  let output=Command::new("node").arg(directory.join("test.mjs")).output().unwrap();fs::write(directory.join(format!("{mode}.log")),[output.stdout.as_slice(),output.stderr.as_slice()].concat()).unwrap();assert!(output.status.success(),"{mode}: {}",String::from_utf8_lossy(&output.stderr));println!("{mode}: {}",String::from_utf8_lossy(&output.stdout));
 }
 println!("retained at {}",directory.display());
}
