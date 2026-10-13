import assert from 'node:assert/strict';import fs from 'node:fs';
import * as ordinary from './runtime-ordinary.mjs';
const files=['runtime-flat.mjs',...fs.readdirSync(new URL('.',import.meta.url)).filter(n=>/^runtime-one-\d+\.mjs$/.test(n))];
const outcome=fn=>{try{return {value:fn()};}catch(e){return {error:e.message};}};
for(const file of files){
 const flat=await import('./'+file),inputs=[{Ok:5},{Err:{Ok:'雪'}},{Err:{Err:true}}];
 for(const x of inputs)for(const name of ['identity','mapped','errors','folded','wide']){
  const expected=ordinary.callOwned(name,[x]);assert.deepEqual(flat.call(name,[x]),ordinary.call(name,[x]));assert.deepEqual(flat.callOwned(name,[x]),expected);
  const binding=flat.compiledValues[name],handle=binding.params[0].capture(x),reply=flat.callCompiled(name,[handle]);assert.deepEqual(binding.result.materialize(reply),expected);
  const ordinaryEntry=ordinary.compiledValues[name],ordinaryReply=ordinary.callCompiled(name,[ordinaryEntry.params[0].capture(x)]);
  for(const limits of [{nodes:0},{nodes:1},{nodes:2},{nodes:4},{depth:0},{depth:1},{depth:2},{bytes:4},{bytes:9},{bytes:50},{}]){
   assert.deepEqual(outcome(()=>binding.result.materialize(reply,limits)),outcome(()=>ordinaryEntry.result.materialize(ordinaryReply,limits)));
   for(const compact of [false,true]){const encoded=compact?'encodeCompact':'encode',decoded=compact?'decodeCompact':'decode';assert.deepEqual(outcome(()=>binding.result[encoded](reply,limits)),outcome(()=>ordinaryEntry.result[encoded](ordinaryReply,limits)));const bytes=ordinaryEntry.result[encoded](ordinaryReply);assert.deepEqual(outcome(()=>binding.result[decoded](bytes,limits)&&true),outcome(()=>ordinaryEntry.result[decoded](bytes,limits)&&true));}
  }
  for(const limits of [{nodes:0},{nodes:1},{nodes:2},{nodes:4},{nodes:100},{depth:0},{depth:1},{depth:128},{bytes:40},{bytes:50},{bytes:1000}])assert.deepEqual(outcome(()=>binding.params[0].capture(x,limits)&&true),outcome(()=>ordinary.compiledValues[name].params[0].capture(x,limits)&&true));
  for(const compact of [false,true]){const a=flat.valueCodecs[name],b=ordinary.valueCodecs[name],request=(compact?a.arguments.compact():a.arguments).encode({x});assert.deepEqual((compact?flat.callCompact:flat.callBinary)(name,request),(compact?ordinary.callCompact:ordinary.callBinary)(name,request));assert.deepEqual(compact?binding.result.encodeCompact(reply):binding.result.encode(reply),(compact?b.result.compact():b.result).encode(expected));}
 }
 for(const x of [{Ok:{Ok:3}},{Ok:{Err:{Ok:true}}},{Ok:{Err:{Err:{Int:'-999999999999999999999999'}}}},{Err:{Ok:'tail'}},{Err:{Err:false}}])assert.deepEqual(flat.callOwned('left',[x]),ordinary.callOwned('left',[x]));
 const leafCases=[{Ok:{F32Bits:0x80000000}},{Ok:{F32Bits:0x7fc01234}},{Err:{Ok:{Int:'-999999999999999999999999'}}},{Err:{Err:{payload:{Ok:{}}}}},{Err:{Err:{payload:{Err:{item0:[{payload:{Ok:{}}}]}}}}}];
 for(const x of leafCases){
  const expected=ordinary.callOwned('leafs',[x]);assert.deepEqual(flat.callOwned('leafs',[x]),expected);
  assert.equal(flat.callOwned('equal',[x,x]),ordinary.callOwned('equal',[x,x]));
  const entry=flat.compiledValues.leafs,h=entry.params[0].capture(x),reply=flat.callCompiled('leafs',[h]);assert.deepEqual(entry.result.materialize(reply),expected);
  for(const compact of [false,true]){const a=flat.valueCodecs.leafs,b=ordinary.valueCodecs.leafs,request=(compact?a.arguments.compact():a.arguments).encode({x});assert.deepEqual((compact?flat.callCompact:flat.callBinary)('leafs',request),(compact?ordinary.callCompact:ordinary.callBinary)('leafs',request));assert.deepEqual(compact?entry.result.encodeCompact(reply):entry.result.encode(reply),(compact?b.result.compact():b.result).encode(expected));}
 }
 for(const invalid of [null,{},[],{Ok:1,Err:false},{Err:{}},{Err:{Ok:1}},{Err:{Err:3}},{Ok:-1},{Ok:4294967296}])assert.deepEqual(outcome(()=>flat.compiledValues.identity.params[0].capture(invalid)),outcome(()=>ordinary.compiledValues.identity.params[0].capture(invalid)));
 for(const limits of [{nodes:0},{nodes:1},{nodes:2},{depth:0},{depth:1},{bytes:5},{bytes:50},{}]){
  const trace=m=>{let log=[];const tail={get Ok(){log.push('leaf');return '雪';}},root={get Err(){log.push('tail');return tail;}};return {result:outcome(()=>m.compiledValues.identity.params[0].capture(root,limits)&&true),log};};assert.deepEqual(trace(flat),trace(ordinary));
 }
 const destination=[];assert.equal(flat.callInto('many',[inputs[2]],destination),2);assert.deepEqual(destination,[inputs[2],inputs[2]]);
 const a=flat.createState(),b=ordinary.createState();
 const shared=a.invoke('shared',[]).result;assert.strictEqual(shared.whole.Err,shared.tail);shared.tail.Ok[0]=99;assert.equal(a.invoke('shared',[]).result.tail.Ok[0],1);
 for(const x of inputs)for(const name of ['nested','outer']){assert.deepEqual(a.invoke(name,[0,x]),b.invoke(name,[0,x]));assert.deepEqual(a.invoke('get',[0]),b.invoke('get',[0]));assert.deepEqual(await a.checkpoint(),await b.checkpoint());}
 for(const x of inputs)for(const fail of [false,true]){const args=[0,{value:x},fail];assert.deepEqual(a.invoke('put',args),b.invoke('put',args));assert.deepEqual(a.invoke('get',[0]),b.invoke('get',[0]));assert.deepEqual(await a.checkpoint(),await b.checkpoint());}
 const snapshot=await b.checkpoint();await a.restore(snapshot);assert.deepEqual(await a.checkpoint(),snapshot);
 a.beginTransaction();b.beginTransaction();a.invoke('put',[1,{value:inputs[0]},false]);b.invoke('put',[1,{value:inputs[0]},false]);a.rollbackTransaction();b.rollbackTransaction();assert.deepEqual(await a.checkpoint(),await b.checkpoint());
 for(const limits of [{values:0},{values:2},{values:10},{depth:0},{depth:2},{depth:128},{bytes:200}]){const result=async f=>{try{return await f();}catch(e){return e.message;}};assert.deepEqual(await result(()=>a.checkpoint(limits)),await result(()=>b.checkpoint(limits)));}
 console.log(file,'preserves values, resources, nested failures, events, versions and exact checkpoints');
}
