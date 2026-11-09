use ink_core::{core::CheckedModule, syntax::parse};
use std::{fs, process::Command};

// values() returns the table's cached ordered rows without copying them. No
// list handed out may change afterwards: not one the host holds, a keep, a
// subscription's last value, or a list the program extends.
const SOURCE: &str = r#"module ordered;
record Row {id:u64,title:String,amount:u64,open:Bool,}
state Rows:Table<u64,Row>=Table.empty();
keep All:List<Row>=Rows.values();
fn extend(xs:List<Row>,row:Row)->List<Row>{return xs.concat([row]);}
fn fold_rows(xs:List<Row>,row:Row)->List<Row>{return foldl(xs,[row],fn(r)=>fn(a)=>a.concat([r]));}
fn window(xs:List<Row>,offset:u32,limit:u32)->List<Row>{return xs.drop(offset).take(limit);}
change put(row:Row)->Result<Unit,String> reads(Rows) writes(Rows){Rows.put(row.id,row);return Ok(());}
change remove(id:u64)->Result<Unit,String> reads(Rows) writes(Rows){let gone=Rows.remove(id);return Ok(());}
change failing(row:Row)->Result<Unit,String> reads(Rows) writes(Rows){Rows.put(row.id,row);return Err("no");}
change set_amount(id:u64,amount:u64)->Result<Unit,String> reads(Rows) writes(Rows){let row=Rows.get(id).unwrap_or(Row{id:id,title:"",amount:0,open:false});Rows.put(id,Row{id:row.id,title:row.title,amount:amount,open:row.open});return Ok(());}
change staged(row:Row)->Result<u64,String> reads(Rows) writes(Rows){let before=Rows.values();Rows.put(row.id,row);return Ok(sum(Rows.values().map(fn(r)=>r.amount))*1000+sum(before.map(fn(r)=>r.amount)));}
change twice(a:Row,b:Row)->Result<u64,String> reads(Rows) writes(Rows){let before=Rows.values();Rows.put(a.id,a);Rows.put(b.id,b);let after=Rows.values();return Ok(sum(after.map(fn(r)=>r.amount))*1000+sum(before.map(fn(r)=>r.amount)));}
query values()->List<Row> reads(Rows){return Rows.values();}
query all()->List<Row> reads(All){return All;}
query page(offset:u32,limit:u32)->List<Row> reads(Rows){return window(Rows.values(),offset,limit);}
query grown(row:Row)->List<Row> reads(Rows){let xs=Rows.values();let ys=xs.concat([row]);return ys.concat([row]);}
query extended(row:Row)->List<Row> reads(Rows){return extend(Rows.values(),row);}
query folded(row:Row)->List<Row> reads(Rows){return fold_rows(Rows.values(),row);}
"#;

#[test]
fn shared_values_never_change_after_they_are_handed_out() {
    let checked = CheckedModule::from_source(parse(SOURCE).unwrap()).unwrap();
    let code = ink_lowering_js::lower(&checked).unwrap().javascript;
    let dir = std::env::temp_dir().join(format!("ink-ordered-rows-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("program.mjs"), &code).unwrap();
    fs::write(dir.join("run.mjs"), r#"
import assert from 'node:assert/strict';import{createState}from'./program.mjs';
const state=createState(),q=(n,a=[])=>state.invoke(n,a).result;
let seed=5;const random=n=>{seed=(Math.imul(seed,1664525)+1013904223)>>>0;return seed%n;};
const row=()=>{const id=BigInt(random(50));return {id,title:'r'+random(3),amount:BigInt(random(9)),open:random(2)===0};};
let model=new Map();
const sorted=m=>[...m.values()].sort((a,b)=>a.id<b.id?-1:a.id>b.id?1:0);
const json=v=>JSON.stringify(v,(_,v)=>typeof v==='bigint'?String(v):v);
const seen=[];const stop=state.subscribe('all',[],v=>seen.push(json(v)));
const held=[];
const check=()=>{
 const want=sorted(model);assert.deepEqual(q('all'),want);
 const values=q('values');assert.deepEqual(values,want);
 const off=random(8),lim=random(8);assert.deepEqual(q('page',[off,lim]),want.slice(off,off+lim));
 const extra={id:999n,title:'x',amount:1n,open:true};
 assert.deepEqual(q('grown',[extra]),[...want,extra,extra]);assert.deepEqual(q('extended',[extra]),[...want,extra]);
 assert.deepEqual(q('folded',[extra]),[extra,...want]);
 // Extending values() inside the program changed nothing that later reads see.
 assert.deepEqual(q('values'),want);
 held.push([values,json(values)]);
};
for(let turn=0;turn<700;turn++){
 const op=random(13),r=row();
 if(op<4){state.invoke('put',[r]);model.set(r.id,r);}
 else if(op<6){state.invoke('remove',[r.id]);model.delete(r.id);}
 else if(op===6){state.invoke('set_amount',[r.id,r.amount]);const old=model.get(r.id)??{id:r.id,title:'',amount:0n,open:false};model.set(r.id,{...old,amount:r.amount});}
 else if(op===7)assert.equal(state.invoke('failing',[r]).committed,false);
 else if(op===8){
  // A staged single cell: values() before and after its own write.
  const before=sorted(model).reduce((n,x)=>n+x.amount,0n);model.set(r.id,r);const after=sorted(model).reduce((n,x)=>n+x.amount,0n);
  assert.equal(state.invoke('staged',[r]).result.Ok,after*1000n+before);
 }
 else if(op===10&&model.size){
  // Two writes in one action over rows it already read: what it read stays.
  const ids=[...model.keys()],a={...model.get(ids[random(ids.length)]),amount:BigInt(random(9))},b={...model.get(ids[random(ids.length)]),amount:BigInt(random(9))};
  const before=sorted(model).reduce((n,x)=>n+x.amount,0n);model.set(a.id,a);model.set(b.id,b);const after=sorted(model).reduce((n,x)=>n+x.amount,0n);
  assert.equal(state.invoke('twice',[a,b]).result.Ok,after*1000n+before);
 }
 else if(op===9){
  const before=q('values'),saved=new Map(model);state.beginTransaction();
  state.invoke('put',[r]);model.set(r.id,r);const s=row();state.invoke('set_amount',[s.id,7n]);if(model.has(s.id))model.set(s.id,{...model.get(s.id),amount:7n});else model.set(s.id,{id:s.id,title:'',amount:7n,open:false});
  assert.deepEqual(q('values'),sorted(model));
  if(random(2)){state.rollbackTransaction();model=saved;assert.deepEqual(q('all'),before);assert.deepEqual(q('values'),before);}else state.commitTransaction();
 }
 else check();
 if(random(4)===0)check();
}
check();
const saved=await state.checkpoint();
state.invoke('put',[{id:1000n,title:'later',amount:3n,open:true}]);
const after=q('values');assert.equal(after.length,model.size+1);
await state.restore(saved);check();
await Promise.resolve();await Promise.resolve();
assert.equal(seen.at(-1),json(sorted(model)),'the subscription saw the last committed rows');
stop();
// Every list returned earlier is exactly as it was when it was returned.
for(const [value,text] of held)assert.equal(json(value),text);
assert.ok(held.length>100,'checks '+held.length);
"#).unwrap();
    let out = Command::new("node").arg(dir.join("run.mjs")).output().unwrap();
    assert!(out.status.success(), "{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    fs::remove_dir_all(&dir).ok();
}
