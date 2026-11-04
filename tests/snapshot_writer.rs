use std::{fs, process::Command};

// The buffered snapshot writer must produce exactly the bytes, and raise
// exactly the limit errors, of the original one-byte-at-a-time writer.
#[test]
fn buffered_snapshot_writer_matches_the_byte_list_reference() {
    let script = r#"
const assert=require('node:assert/strict');
function reference(limits){
 const out=[];let left=limits.values;
 function bytes(xs){if(xs.length>limits.bytes-out.length)throw Error('snapshot byte limit exceeded');for(const x of xs)out.push(x);}
 function integer(n,size){n=BigInt(n);for(let i=0;i<size;i++){bytes([Number(n&255n)]);n>>=8n;}}
 function raw(xs){integer(xs.length,8);bytes(xs);}
 function value(v,t){if(--left<0)throw Error('snapshot value/depth limit exceeded');switch(t.kind){
  case 'bool':bytes([v?1:0]);break;case 'u32':case 'i32':integer(v,4);break;case 'u64':integer(v,8);break;
  case 'string':raw(new TextEncoder().encode(v));break;case 'id':integer(BigInt('0x'+v),16);break;
  case 'record':for(const f of t.fields)value(v[f.name],f.type);break;
  case 'list':integer(v.length,8);for(const x of v)value(x,t.element);break;
  case 'option':if(Object.hasOwn(v,'Some')){bytes([1]);value(v.Some,t.element);}else bytes([0]);break;
 }}
 return {value,bytes,finish:()=>new Uint8Array(out)};
}
let seed=99;const random=n=>{seed=(Math.imul(seed,1664525)+1013904223)>>>0;return seed%n;};
const strings=['','a','é','雪','😀','\ud800x','x'.repeat(300),'混合 text 😀'.repeat(20)];
const row={kind:'record',fields:[{name:'id',type:{kind:'u64'}},{name:'n',type:{kind:'u32'}},{name:'d',type:{kind:'i32'}},{name:'s',type:{kind:'string'}},{name:'o',type:{kind:'option',element:{kind:'string'}}},{name:'b',type:{kind:'bool'}},{name:'h',type:{kind:'id'}}]};
const type={kind:'list',element:row};
const make=()=>({id:[0n,1n,255n,256n,18446744073709551615n,BigInt(random(1e9))*4294967296n][random(6)],n:[0,1,255,65536,4294967295][random(5)],d:[-1,-2147483648,2147483647,0,7][random(5)],s:strings[random(strings.length)],o:random(2)?{Some:strings[random(strings.length)]}:{None:null},b:random(2)===0,h:random(16).toString(16).repeat(32)});
const run=(w,v)=>{try{w.value(v,type);w.bytes([1,2,3]);return w.finish();}catch(e){return e.message;}};
for(let turn=0;turn<400;turn++){
 const v=Array.from({length:random(12)},make),limits={values:1000000,depth:128,bytes:random(4)?1<<20:random(400)};
 const a=run(reference(limits),v),b=run(__snapshotWriter(limits),v);
 assert.deepEqual(b,a);
}
"#;
    let path = std::env::temp_dir().join(format!("ink-snapshot-writer-{}.cjs", std::process::id()));
    fs::write(&path, format!("const __encoder=new TextEncoder();\n{}\n{script}", include_str!("../src/snapshot.mjs"))).unwrap();
    let result = Command::new("node").arg(&path).output().unwrap();
    fs::remove_file(path).unwrap();
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
}
