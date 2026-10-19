import assert from 'node:assert/strict';
import fs from 'node:fs';
import * as m from './program.mjs';
const mode=process.argv[2],cases=JSON.parse(fs.readFileSync(new URL('cases.json',import.meta.url)));
let calls=0;
for(const c of cases){
 const entry=m.compiledValues[c.name],args=c.args.map((v,i)=>entry.params[i].capture(v));
 const expected=m.valueCodecs[c.name].result.encode(c.expected);
 assert.deepEqual(entry.result.encode(m.callCompiled(c.name,args)),expected,`${mode}:${c.name}`);calls++;
 for(const fixed of [[],[0]]){
  const token=m.prepareCompiled(c.name,fixed.map(i=>[i,args[i]]));
  for(let repeat=0;repeat<2;repeat++){
   assert.deepEqual(entry.result.encode(m.callPreparedCompiled(token,args.filter((_,i)=>!fixed.includes(i)))),expected,`${mode}:${c.name}:prepared`);calls++;
  }
 }
}
// Public pure-only field reads retain their original demand, multiplicity,
// throwing behavior and normalization. Stateful modules capture at ingress.
let reads=[];
const pair={get a(){reads.push('a');return reads.length;},get b(){reads.push('b');throw Error('unused');}};
if(mode==='borrowed'){
 assert.equal(m.functions.twice(pair),14);assert.deepEqual(reads,['a','a','a','a']);
 reads=[];assert.throws(()=>m.functions.strict_unused(pair),/unused/);assert.deepEqual(reads,['b']);
}else{
 assert.equal(m.functions.twice(pair),2);assert.deepEqual(reads,['a']);
 reads=[];assert.throws(()=>m.functions.strict_unused(pair),/unused/);assert.deepEqual(reads,['a','b']);
}
console.log(JSON.stringify({mode,cases:cases.length,calls,exactTypedBytes:true,borrowedGetters:true}));
