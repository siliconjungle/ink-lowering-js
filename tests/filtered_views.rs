use ink_core::{core::CheckedModule, syntax::parse};
use std::{fs, process::Command};

const SOURCE: &str = r#"module views;
record Row {id:u64,title:String,amount:u64,open:Bool,}
state Rows:Table<u64,Row>=Table.empty();
state Version:u64=0;
keep OpenTotal:u64=sum(Rows.values().filter(fn(row)=>row.open).map(fn(row)=>row.amount));
change put(row:Row)->Result<Unit,String> reads(Rows) writes(Rows){Rows.put(row.id,row);return Ok(());}
change remove(id:u64)->Result<Unit,String> reads(Rows) writes(Rows){let gone=Rows.remove(id);return Ok(());}
change failing(row:Row)->Result<Unit,String> reads(Rows) writes(Rows){Rows.put(row.id,row);return Err("no");}
change bump()->Result<Unit,String> reads(Version) writes(Version){Version=Version+1;return Ok(());}
change staged(row:Row)->Result<u64,String> reads(Rows,Version) writes(Rows,Version){Rows.put(row.id,row);let n=sum(Rows.values().filter(fn(row)=>row.open).map(fn(row)=>1));Version=Version+1;return Ok(n);}
change peek(row:Row)->Result<u64,String> reads(Rows) writes(Rows){Rows.put(row.id,row);return Ok(sum(Rows.values().filter(fn(row)=>row.open).map(fn(row)=>1)));}
query open()->List<Row> reads(Rows){return Rows.values().filter(fn(row)=>row.open);}
query big()->List<Row> reads(Rows){return Rows.values().filter(fn(row)=>row.amount>=5&&row.open);}
query scan(flag:Bool)->List<Row> reads(Rows){return Rows.values().filter(fn(row)=>row.open==flag);}
query one(id:u64)->Option<Row> reads(Rows){return Rows.get(id);}
query total()->u64 reads(OpenTotal,Version){return OpenTotal+Version;}
query nested()->List<Row> reads(Rows){return open();}
"#;

#[test]
fn filtered_views_match_scans_and_commits_report_the_paths_they_change() {
    let checked = CheckedModule::from_source(parse(SOURCE).unwrap()).unwrap();
    let emission = ink_lowering_js::lower(&checked).unwrap();
    let code = emission.javascript;
    assert!(code.contains("__filtered(ctx,rt,0,0)"), "the open filter reads a maintained view");
    assert!(!code.contains("__filtered(ctx,rt,0,2)"), "a predicate that captures a parameter scans");
    let dir = std::env::temp_dir().join(format!("ink-filtered-views-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("program.mjs"), format!("{code}\nexport const stateMetadata=__stateMetadata;\n")).unwrap();
    fs::write(dir.join("run.mjs"), r#"
import assert from 'node:assert/strict';import{createState,stateMetadata as metadata}from'./program.mjs';
const deps=Object.fromEntries(metadata.actions.map(a=>[a.name,a.deps]));
assert.deepEqual(metadata.filters.map(f=>f.path),['Rows#0','Rows#1']);
assert.deepEqual(deps.open,['Rows#0']);assert.deepEqual(deps.big,['Rows#1']);assert.deepEqual(deps.scan,['Rows']);
assert.deepEqual(deps.one,['Rows[0]']);assert.deepEqual(deps.total,['Rows#0','Version']);assert.deepEqual(deps.nested,['Rows#0']);
assert.deepEqual(metadata.keeps[0].deps,['Rows#0']);assert.equal(metadata.actions.find(a=>a.name==='peek').singleCell,true);
const state=createState(),q=(n,a=[])=>state.invoke(n,a).result;
let seed=7;const random=n=>{seed=(Math.imul(seed,1664525)+1013904223)>>>0;return seed%n;};
const row=()=>{const id=BigInt(random(60));return {id,title:'r'+random(3),amount:BigInt(random(9)),open:random(2)===0};};
const check=()=>{
 const open=q('scan',[true]);assert.deepEqual(q('open'),open);assert.deepEqual(q('nested'),open);
 assert.deepEqual(q('big'),open.filter(r=>r.amount>=5n));
 assert.equal(q('total'),open.reduce((n,r)=>n+r.amount,0n)+q('version'));
};
// `version` is a test-only view of Version through a query that scans nothing.
for(let turn=0;turn<600;turn++){
 const op=random(10),r=row();
 if(op<5)state.invoke('put',[r]);
 else if(op<7)state.invoke('remove',[r.id]);
 else if(op===7)assert.equal(state.invoke('failing',[r]).committed,false);
 else if(op===8&&random(2)){const out=state.invoke('peek',[r]).result.Ok;assert.equal(out,BigInt(q('scan',[true]).length));}
 else if(op===8){const out=state.invoke('staged',[r]).result.Ok;assert.equal(out,BigInt(q('scan',[true]).length));}
 else{
  // Host transactions: views follow writes inside them and their rollback.
  const before=q('open');state.beginTransaction();state.invoke('put',[r]);state.invoke('remove',[row().id]);
  if(random(2)){state.rollbackTransaction();assert.deepEqual(q('open'),before);}else state.commitTransaction();
 }
 if(random(2))check();
}
check();
const saved=await state.checkpoint();
state.invoke('put',[{id:999n,title:'x',amount:1n,open:true}]);check();
await state.restore(saved);check();assert.equal(q('one',[999n]).None,null);

// Change reports.
state.trackChanges();
const take=()=>state.takeChanges();
state.invoke('put',[{id:1000n,title:'a',amount:7n,open:true}]);
assert.deepEqual(take(),{all:false,roots:['Rows'],filters:[0,1],keys:{Rows:[1000n]}});
state.invoke('put',[{id:1001n,title:'b',amount:7n,open:false}]);
assert.deepEqual(take(),{all:false,roots:['Rows'],filters:[],keys:{Rows:[1001n]}},'a closed row changes no view');
state.invoke('put',[{id:1001n,title:'b',amount:7n,open:false}]);
assert.deepEqual(take(),{all:false,roots:[],filters:[],keys:{}},'an identical rewrite changes nothing');
state.invoke('put',[{id:1000n,title:'a',amount:2n,open:true}]);
assert.deepEqual(take().filters,[0,1],'leaving a view changes it');
state.invoke('failing',[{id:5000n,title:'f',amount:9n,open:true}]);
assert.deepEqual(take().roots,[],'a failed action reports nothing');
state.invoke('bump',[]);
const bumped=take();assert.deepEqual([bumped.roots,bumped.filters],[['Version'],[]]);
state.invoke('put',[{id:1002n,title:'c',amount:1n,open:true}]);state.invoke('remove',[1002n]);
assert.deepEqual(take().roots,[],'a row added and removed between reports is unchanged');
const token=state.changeToken();state.invoke('put',[{id:1003n,title:'d',amount:1n,open:true}]);
assert.deepEqual(take().keys,{Rows:[1003n]});assert.equal(state.changesSince(token).changes.length,1,'change tokens still see their log');
state.releaseChangeToken(token);state.invoke('remove',[1003n]);assert.deepEqual(take().keys,{Rows:[1003n]});
await state.restore(saved);assert.equal(take().all,true,'a restore reports everything');check();
"#.replace("q('version')", "BigInt(state.invoke('total',[]).result-q('scan',[true]).reduce((n,r)=>n+r.amount,0n))")).unwrap();
    let output = Command::new("node").arg(dir.join("run.mjs")).output().unwrap();
    fs::remove_dir_all(&dir).unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}
