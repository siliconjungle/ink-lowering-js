use std::{fs,process::Command};

#[test]
fn column_event_log_preserves_order_truncation_acknowledgement_and_wide_values(){
 let dir=std::env::temp_dir().join(format!("ink-event-columns-{}",std::process::id()));fs::create_dir_all(&dir).unwrap();
 let fixture=r#"
import assert from 'node:assert/strict';
for(const channels of [['only'],['a','b']]){
 const log=new __EventColumns(channels),reference=[];
 for(let i=0;i<12000;i++){const e={commit:BigInt(i+1)+(1n<<53n),position:BigInt(i%5),channel:channels[i%channels.length],value:i%2?{nested:[BigInt(i)]}:18446744073709551615n};log.push(e);reference.push(e);}
 assert.equal(log.length,reference.length);assert.deepEqual([...log],reference);assert.deepEqual(log.at(-1),reference.at(-1));
 assert.deepEqual(log.map(e=>e.value),reference.map(e=>e.value));
 const tail=log.filter(e=>e.commit>reference[100].commit||e.commit===reference[100].commit&&e.position>reference[100].position);
 assert.deepEqual([...tail],reference.slice(101));
 const mark=log.length;log.append(18446744073709551615n,18446744073709551615n,channels[0],{nested:[1n]});log.length=mark;
 assert.deepEqual([...log],reference);assert.deepEqual([...tail],reference.slice(101));
 log.length=17;log.append(99n,0n,channels[0],3n);assert.deepEqual([...log],[...reference.slice(0,17),{commit:99n,position:0n,channel:channels[0],value:3n}]);
 assert.throws(()=>{log.length=100;});log.length=0;log.append(2n,0n,channels[0],4n);assert.equal(log.length,1);assert.equal(log.at(1),undefined);assert.deepEqual(log.at(0),{commit:2n,position:0n,channel:channels[0],value:4n});
 const restored=__eventLog({eventColumns:true,events:channels.map(name=>({name}))},reference);assert.deepEqual([...restored],reference);
}
"#;
 fs::write(dir.join("run.mjs"),format!("{}\n{fixture}",include_str!("../src/event_columns.mjs"))).unwrap();
 let out=Command::new("node").arg(dir.join("run.mjs")).output().unwrap();fs::remove_dir_all(dir).unwrap();assert!(out.status.success(),"{}",String::from_utf8_lossy(&out.stderr));
}
