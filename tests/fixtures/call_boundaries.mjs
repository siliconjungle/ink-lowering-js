import assert from 'node:assert/strict';
import fs from 'node:fs';
import * as m from './program.mjs';
const mode=process.argv[2],wide=mode==='wide',cases=JSON.parse(fs.readFileSync(new URL('cases.json',import.meta.url)));
assert.equal(m.functions.scalar.length,2);
assert.equal(m.functions.shift.length,0,'collection public calls keep the established variadic adapter');
let calls=0;
for(const c of cases){
 const codec=m.compiledValues[c.name],handles=c.args.map((v,i)=>codec.params[i].capture(v));
 const wire=m.valueCodecs[c.name].result,expected=wire.encode(c.expected);
 const ordinary=m.functions[c.name](...c.args),retained=wire.encode(ordinary);
 assert.deepEqual(retained,expected,c.name);calls++;
 if(!Array.isArray(c.expected))continue;
 for(const indices of [[],[0],handles.map((_,i)=>i)]){
  const token=m.prepareCompiled(c.name,indices.map(i=>[i,handles[i]])),args=handles.filter((_,i)=>!indices.includes(i));
  const out=new Array(c.expected.length+3).fill('tail');
  for(let i=0;i<3;i++){
   const count=m.callPreparedCompiledInto(token,args,out);
   assert.equal(count,c.expected.length);assert.deepEqual(wire.encode(out.slice(0,count)),expected,c.name);
   assert.deepEqual(out.slice(count),['tail','tail','tail']);
   if(out[0]&&typeof out[0]==='object'){if(Array.isArray(out[0]))out[0].push(99);else if('items'in out[0])out[0].items.push(99);else for(const key of Object.keys(out[0]))out[0][key]=0;}
   assert.deepEqual(codec.result.encode(m.callPreparedCompiled(token,args)),expected,'private inputs/caches must survive destination mutation');calls++;
  }
  assert.deepEqual(wire.encode(ordinary),retained,'ordinary output retained');
 }
}
// Changing inputs, typed output storage and untouched tails. Preparation owns
// captured data; mutation of the original host array cannot alter it.
const c=m.compiledValues.shift,host=[1,2,3],h=c.params[0].capture(host),t=m.prepareCompiled('shift',[[0,h]]);host[0]=999;
const out=wide?new Float64Array(6):new Float32Array(6);out.fill(123);
for(const amount of [0,-0,.5,-2]){const n=m.callPreparedCompiledInto(t,[c.params[1].capture(amount)],out);assert.equal(n,3);assert.deepEqual([...out],[1+amount,2+amount,3+amount,123,123,123]);}
// The ordinary scalar-list path writes its terminal stage without allocating
// the intermediate result array. Handle/configuration setup is outside this probe.
const dynamic=[c.params[1].capture(.5)],NativeArray=globalThis.Array;let arrays=0;
globalThis.Array=new Proxy(NativeArray,{construct(target,args){arrays++;return Reflect.construct(target,args,target);}});
try{m.callPreparedCompiledInto(t,dynamic,out);assert.equal(arrays,0);m.callPreparedCompiled(t,dynamic);assert.equal(arrays,1);}finally{globalThis.Array=NativeArray;}
if(mode==='native'){assert.throws(()=>m.functions.scalar(.75,-.25,'unused'),TypeError);assert.equal(m.functions.word_float(3,'unused'),3);}
const NativeWeakMap=globalThis.WeakMap;let memos=0;
globalThis.WeakMap=new Proxy(NativeWeakMap,{construct(target,args){memos++;return Reflect.construct(target,args,target);}});
try{assert.equal(m.functions.scalar(.75,-.25),.5625);assert.equal(memos,0);}finally{globalThis.WeakMap=NativeWeakMap;}
assert.throws(()=>m.callPreparedCompiledInto({},[],out),/invalid prepared/);
assert.throws(()=>m.callPreparedCompiledInto(t,[],out),/invalid prepared/);
assert.throws(()=>m.callPreparedCompiledInto(t,[{}],out),/type or owner mismatch/);
const foreign=await import('./program.mjs?foreign');
assert.throws(()=>foreign.callPreparedCompiledInto(t,[c.params[1].capture(0)],out),/invalid prepared/);
assert.throws(()=>m.callPreparedCompiledInto(m.prepareCompiled('scalar',[]),[m.compiledValues.scalar.params[0].capture(1),m.compiledValues.scalar.params[1].capture(2)],out),/invalid prepared/);
if(!wide){
const checked=m.compiledValues.checked,guard=m.prepareCompiled('checked',[[0,checked.params[0].capture([1,2])]]),destination=[9,9,9];
for(let i=0;i<2;i++){assert.throws(()=>m.callPreparedCompiledInto(guard,[checked.params[1].capture(false)],destination),/precondition/);assert.deepEqual(destination,[9,9,9]);}
assert.equal(m.callPreparedCompiledInto(guard,[checked.params[1].capture(true)],destination),2);assert.deepEqual(destination,[1,2,9]);
}
const destination=[9,9,9];
// Stable index construction remains cached in residual destination calls, and
// a throwing construction retries. Output reuse must not expose cached values.
const indexed=m.compiledValues.indexed,key=indexed.params[0].capture([1,2,7]),token=m.prepareCompiled('indexed',[[0,key]]);
const NativeMap=globalThis.Map;let attempts=0;const sentinel=new Error('retry');
globalThis.Map=new Proxy(NativeMap,{construct(target,args){if(++attempts===1)throw sentinel;return Reflect.construct(target,args,target);}});
try{
 assert.throws(()=>m.callPreparedCompiledInto(token,[indexed.params[1].capture([7,9])],destination),e=>e===sentinel);
 for(const xs of [[7,9],[],[1,2,99]]){const count=m.callPreparedCompiledInto(token,[indexed.params[1].capture(xs)],destination);assert.deepEqual(destination.slice(0,count),xs.map(x=>[1,2,7].includes(x)));}
 assert.equal(attempts,2);
}finally{globalThis.Map=NativeMap;}
// Conversion memoization still captures a repeated raw tag once per invocation
// in the eager/captured lanes, including when the tag is used twice.
if(mode==='captured'||mode==='bits'){
 let reads=0;const tag={get F32Bits(){reads++;return 0x7fc12345;}};
 m.functions.pair(tag,tag);assert.equal(reads,1);m.functions.pair(tag,tag);assert.equal(reads,2);
 const result=[];m.functionsInto.pair(tag,tag,result);assert.equal(reads,3);
}
console.log(JSON.stringify({referenceCases:cases.length,completeCalls:calls,mode,typedDestination:true,privateOwnership:true,contractRetry:true}));
