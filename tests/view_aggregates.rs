use ink_core::{core::CheckedModule, syntax::parse};
use std::{fs, process::Command};

// Each keep sums or counts a maintained view; each `scan_*` query computes the
// same aggregate over a filter that captures its parameter, which scans.
const SOURCE: &str = r#"module aggregates;
record Row {id:u64,open:Bool,a:u64,b:u32,c:i32,d:Int,f:f32,z:u64,}
state Rows:Table<u64,Row>=Table.empty();
keep A:u64=sum(Rows.values().filter(fn(row)=>row.open).map(fn(row)=>row.a));
keep B:u32=sum(Rows.values().filter(fn(row)=>row.open).map(fn(row)=>row.b));
keep C:i32=sum(Rows.values().filter(fn(row)=>row.open).map(fn(row)=>row.c));
keep D:Int=sum(Rows.values().filter(fn(row)=>row.open).map(fn(row)=>row.d));
keep F:f32=sum(Rows.values().filter(fn(row)=>row.open).map(fn(row)=>row.f));
keep N:Int=count(Rows.values().filter(fn(row)=>row.open));
keep Q:u64=sum(Rows.values().filter(fn(row)=>row.open).map(fn(row)=>choose(row.z==0,7,row.a*row.z)));
keep T:u64=sum(Rows.values().filter(fn(row)=>row.open).map(fn(row)=>twice(row.a)));
fn twice(x:u64)->u64{return x+x;}
change put(row:Row)->Result<Unit,String> reads(Rows) writes(Rows){Rows.put(row.id,row);return Ok(());}
change remove(id:u64)->Result<Unit,String> reads(Rows) writes(Rows){let gone=Rows.remove(id);return Ok(());}
change failing(row:Row)->Result<Unit,String> reads(Rows) writes(Rows){Rows.put(row.id,row);return Err("no");}
change peek(row:Row)->Result<u64,String> reads(Rows) writes(Rows){Rows.put(row.id,row);return Ok(sum(Rows.values().filter(fn(row)=>row.open).map(fn(row)=>row.a)));}
query a()->u64 reads(A){return A;}
query b()->u32 reads(B){return B;}
query c()->i32 reads(C){return C;}
query d()->Int reads(D){return D;}
query f()->f32 reads(F){return F;}
query n()->Int reads(N){return N;}
query q()->u64 reads(Q){return Q;}
query t()->u64 reads(T){return T;}
query scan_a(flag:Bool)->u64 reads(Rows){return sum(Rows.values().filter(fn(row)=>row.open==flag).map(fn(row)=>row.a));}
query scan_b(flag:Bool)->u32 reads(Rows){return sum(Rows.values().filter(fn(row)=>row.open==flag).map(fn(row)=>row.b));}
query scan_c(flag:Bool)->i32 reads(Rows){return sum(Rows.values().filter(fn(row)=>row.open==flag).map(fn(row)=>row.c));}
query scan_d(flag:Bool)->Int reads(Rows){return sum(Rows.values().filter(fn(row)=>row.open==flag).map(fn(row)=>row.d));}
query scan_f(flag:Bool)->f32 reads(Rows){return sum(Rows.values().filter(fn(row)=>row.open==flag).map(fn(row)=>row.f));}
query scan_n(flag:Bool)->Int reads(Rows){return count(Rows.values().filter(fn(row)=>row.open==flag));}
query scan_q(flag:Bool)->u64 reads(Rows){return sum(Rows.values().filter(fn(row)=>row.open==flag).map(fn(row)=>choose(row.z==0,7,row.a*row.z)));}
query scan_t(flag:Bool)->u64 reads(Rows){return sum(Rows.values().filter(fn(row)=>row.open==flag).map(fn(row)=>twice(row.a)));}
"#;

#[test]
fn view_sums_and_counts_match_scans() {
    let checked = CheckedModule::from_source(parse(SOURCE).unwrap()).unwrap();
    let code = ink_lowering_js::lower(&checked).unwrap().javascript;
    for (name, sum) in [("A", 0), ("B", 1), ("C", 2), ("D", 3), ("Q", 4), ("T", 5)] {
        assert!(code.contains(&format!("__filteredSum(ctx,rt,0,0,{sum})")), "{name} reads a maintained sum");
    }
    assert!(code.contains("__filteredCount(ctx,rt,0,0)"), "N reads the view's length");
    assert_eq!(code.matches("__filteredSum(").count() - 1, 7, "six keeps and the in-action sum; F keeps the scan");
    let dir = std::env::temp_dir().join(format!("ink-view-aggregates-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("program.mjs"), format!("{code}\nexport const stateMetadata=__stateMetadata;\n")).unwrap();
    fs::write(dir.join("run.mjs"), r#"
import assert from 'node:assert/strict';import{createState}from'./program.mjs';
const state=createState();
const outcome=(name,args=[])=>{try{const out=state.invoke(name,args);return out.result;}catch(e){return 'error: '+e.message;}};
let seed=11;const random=n=>{seed=(Math.imul(seed,1664525)+1013904223)>>>0;return seed%n;};
const big=[0n,1n,7n,(1n<<64n)-1n,(1n<<63n)];
const row=()=>({id:BigInt(random(40)),open:random(2)===0,a:big[random(5)],b:[0,1,4294967295,2147483648][random(4)],c:[0,-1,2147483647,-2147483648,5][random(5)],d:[0n,-3n,(1n<<80n),-(1n<<70n)][random(4)],f:[0.1,1e16,-1e16,2.5][random(4)],z:random(6)===0?0n:BigInt(1+random(3))});
let checks=0;
const check=()=>{
 for(const k of ['a','b','c','d','f','n','q','t']){
  const kept=outcome(k),scanned=outcome('scan_'+k,[true]);
  // Floating-point sums keep the scan, so they agree exactly too.
  assert.deepEqual(kept,scanned,k+' after '+checks+' checks');
 }
 checks++;
};
for(let turn=0;turn<800;turn++){
 const op=random(10),r=row();
 if(op<5)state.invoke('put',[r]);
 else if(op<7)state.invoke('remove',[r.id]);
 else if(op===7)assert.equal(state.invoke('failing',[r]).committed,false);
 else if(op===8){const out=state.invoke('peek',[r]).result.Ok;assert.equal(out,outcome('scan_a',[true]));}
 else{
  const before=outcome('a');state.beginTransaction();state.invoke('put',[r]);state.invoke('remove',[row().id]);
  if(random(2)){state.rollbackTransaction();assert.equal(outcome('a'),before);}else state.commitTransaction();
 }
 if(random(3)===0)check();
}
check();
const saved=await state.checkpoint();
state.invoke('put',[{id:999n,open:true,a:5n,b:5,c:5,d:5n,f:5,z:1n}]);check();
await state.restore(saved);check();
assert.ok(checks>200,'checks '+checks);
"#).unwrap();
    let out = Command::new("node").arg(dir.join("run.mjs")).output().unwrap();
    assert!(out.status.success(), "{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    fs::remove_dir_all(&dir).ok();
}
