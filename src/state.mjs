// CALLER CONTRACT: arguments must match declared types; malformed data may throw or corrupt state.
// Fixed state primitives used by emitted action bodies, without an AST evaluator.
class __Abort {constructor(value){this.value=value;}}
function __try(value){if(Object.hasOwn(value,'Err'))throw new __Abort(value.Err);return value.Ok;}
function __nested(value,change){if(change && Object.hasOwn(value,'Err'))throw new __Abort(value.Err);return value;}
function __equal(a,b){if(a===b)return true;if(!a||!b||typeof a!=='object'||typeof b!=='object')return false;const ak=Object.keys(a),bk=Object.keys(b);return ak.length===bk.length&&ak.every(k=>Object.hasOwn(b,k)&&__equal(a[k],b[k]));}
function __checked(op,a,b,bits){const x=BigInt(a),y=BigInt(b),n=op==='checked_add'?x+y:op==='checked_sub'?x-y:x*y;const min=bits===-32?-2147483648n:0n,max=bits===-32?2147483648n:1n<<BigInt(bits);return n<min||n>=max?{Err:'ArithmeticError.Overflow'}:{Ok:Math.abs(bits)===32?Number(n):n};}
function __okOr(value,error){return Object.hasOwn(value,'Some')?{Ok:value.Some}:{Err:error};}
function __mapValue(value,kind,method,fn){
 if(kind==='list')return value[method](fn);
 if(kind==='option')return Object.hasOwn(value,'Some')?{Some:fn(value.Some)}:{None:null};
 const tag=method==='map_err'?'Err':'Ok';return Object.hasOwn(value,tag)?{[tag]:fn(value[tag])}:value;
}
const __encoder=new TextEncoder();
function __tick(ctx){if(--ctx.fuel<0)throw Error('Ink state execution budget exhausted');}
function __keyCompare(a,b,type){
 // For well-formed strings, UTF-8 lexicographic order equals Unicode scalar
 // order. Walk scalars directly, including astral/BMP boundaries, without bytes.
 if(type.kind==='string'){let i=0,j=0;while(i<a.length&&j<b.length){const x=a.codePointAt(i),y=b.codePointAt(j);if(x!==y)return x-y;i+=x>65535?2:1;j+=y>65535?2:1;}return (i<a.length?1:0)-(j<b.length?1:0);}
 return a<b?-1:a>b?1:0;
}
// A table's ordered key view is reused until membership changes. Values are
// fetched at traversal time, so replacement never leaves stale row payloads.
const __tableOrder=new WeakMap();
function __keyPosition(keys,key,type){let low=0,high=keys.length;while(low<high){const middle=(low+high)>>>1;if(__keyCompare(keys[middle],key,type)<0)low=middle+1;else high=middle;}return low;}
function __tableMembership(rt,index,key,present){
 const view=__tableOrder.get(rt.tables[index]);if(!view)return;
 const at=__keyPosition(view.keys,key,rt.meta.roots[index].key);
 // Cancel transient remove/reinsert and insert/remove pairs against the base.
 if((at<view.keys.length&&view.keys[at]===key)===present)view.pending.delete(key);
 else view.pending.set(key,present);
}
function __orderedKeys(rt,index){
 const table=rt.tables[index],type=rt.meta.roots[index].key;let view=__tableOrder.get(table);
 if(!view){view={keys:[...table.keys()].sort((a,b)=>__keyCompare(a,b,type)),pending:new Map()};__tableOrder.set(table,view);}
 if(view.pending.size){
  const changes=[...view.pending.keys()].sort((a,b)=>__keyCompare(a,b,type)),merged=[];let i=0,j=0;
  while(i<view.keys.length||j<changes.length){
   if(j===changes.length||(i<view.keys.length&&__keyCompare(view.keys[i],changes[j],type)<0))merged.push(view.keys[i++]);
   else{const key=changes[j++];if(i<view.keys.length&&view.keys[i]===key)i++;if(view.pending.get(key))merged.push(key);}
  }
  view.keys=merged;view.pending.clear();
 }
 return view.keys;
}
// The first before-image of each cell suffices for whole-transaction rollback.
function __remember(rt,index,key,present,value){if(!rt.seen[index].has(key)){rt.seen[index].add(key);rt.undo.push([index,key,present,value]);}}
// Only scalar keeps are retained. Reverse edges come from checked source effects.
// Before-images preserve cached values across failed actions; host rollback and
// restore discard caches rather than serializing them into logical snapshots.
function __invalidateKeeps(rt,index){
 if(!rt.keeps)return;
 for(const keep of rt.meta.keepInvalidations[index]){
  if(!rt.keepSeen.has(keep)){rt.keepSeen.add(keep);rt.keepUndo.push([keep,rt.keeps[keep]]);}
  rt.keeps[keep]=undefined;
 }
}
function __restoreKeeps(rt){for(let i=rt.keepUndo.length-1;i>=0;i--){const [keep,value]=rt.keepUndo[i];rt.keeps[keep]=value;}}
function __cachedKeep(ctx,rt,index,compute){const prior=rt.keeps[index];if(prior!==undefined)return prior;const value=compute(ctx,rt);rt.keeps[index]=value;return value;}
function __table(rt,index,method,args){
 const table=rt.tables[index],key=args[0];
 if(method==='values'||method==='keys'){const keys=__orderedKeys(rt,index);return method==='keys'?keys.slice():keys.map(k=>table.get(k));}
 if(method==='intersect_table'){const other=rt.tables[args[0]],small=table.size<=other.size?table:other,large=small===table?other:table;const result=[];for(const k of small.keys())if(large.has(k))result.push(k);return result.sort((a,b)=>__keyCompare(a,b,rt.meta.roots[index].key));}
 if(method==='intersect_keys'){const selected=new Set(args[0]);return __orderedKeys(rt,index).filter(k=>selected.has(k));}
 if(method==='put_many'){const keys=args[0],values=args[1];if(keys.length!==values.length)return {Err:'table bulk length mismatch'};for(let i=0;i<keys.length;i++)__table(rt,index,'put',[keys[i],values[i]]);return {Ok:null};}
 if(method==='remove_many'){for(const k of args[0])__table(rt,index,'remove',[k]);return null;}
 // Checked table values never use undefined (Unit uses null), so one lookup
 // distinguishes absence without a second hash probe.
 if(method==='get'){const value=table.get(key);return value===undefined?{None:null}:{Some:value};}
 if(method==='contains')return table.has(key);
 if(method==='remove'){const value=table.get(key),present=value!==undefined;if(present){__invalidateKeeps(rt,index);__tableMembership(rt,index,key,false);__remember(rt,index,key,true,value);table.delete(key);}return present?{Some:value}:{None:null};}
 const prior=table.get(key),present=prior!==undefined;
 if(method==='insert'&&present)throw Error('insert key already present');
 if(method==='replace'&&!present)throw Error('replace key absent');
 if(present&&Object.is(prior,args[1]))return null;
 __invalidateKeeps(rt,index);
 if(!present)__tableMembership(rt,index,key,true);
 __remember(rt,index,key,present,prior);table.set(key,args[1]);return null;
}
function __rollback(rt,mark){for(let i=rt.undo.length-1;i>=mark;i--){const [index,key,present,value]=rt.undo[i];const table=rt.tables[index];if(table.has(key)!==present)__tableMembership(rt,index,key,present);if(present)table.set(key,value);else table.delete(key);}rt.undo.length=mark;}
function __createState(meta,implementations){
 const rt={meta,tables:meta.roots.map(()=>new Map()),version:0n,outbox:[],staged:[],busy:false,undo:[],seen:meta.roots.map(()=>new Set()),host:null,keeps:new Array(meta.keeps.length),keepUndo:[],keepSeen:new Set()};
 const actionMap=new Map(meta.actions.map(a=>[a.name,{...a,inputs:a.params.map(p=>__stateCapture(p.type)),output:__stateResult(a.result)}]));
 const eventOutput=new Map(meta.events.map(e=>[e.name,__stateResult(e.type)]));
 function invoke(name,args){const a=actionMap.get(name);if(!a)throw Error(`unknown action ${name}`);return run(a,args);}
 function read(a,args,inputs,output,resultOnly){
  if(rt.busy)throw Error('state operation already in progress');
  const captured=(inputs||a.inputs).map((convert,i)=>convert(args[i]));
  const ctx={fuel:100000000};
  if(a.precondition && implementations[actionMap.get(a.precondition).index](ctx,rt,...captured)!==true)throw Error('contract precondition failed');
  rt.busy=true;
  try{const result=(output||a.output)(implementations[a.index](ctx,rt,...captured));return resultOnly?result:{result,committed:false,version:rt.version,events:[]};}
  finally{rt.busy=false;}
 }
 function run(a,args,inputs=null,output=null,resultOnly=false){
  if(!a.change)return read(a,args,inputs,output,resultOnly);
  if(rt.busy)throw Error('state operation already in progress');
  const captured=(inputs||a.inputs).map((convert,i)=>convert(args[i]));
  if(a.precondition && implementations[actionMap.get(a.precondition).index]({fuel:100000000},rt,...captured)!==true)throw Error('contract precondition failed');
  const mark=rt.undo.length,version=rt.version,outbox=rt.outbox,outboxLength=rt.outbox.length;
  rt.busy=true;rt.staged.length=0;const ctx={fuel:100000000};
  try{
   const value=implementations[a.index](ctx,rt,...captured);
   const result=(output||a.output)(value);
   if(a.fallible && Object.hasOwn(value,'Err')){__rollback(rt,mark);__restoreKeeps(rt);return resultOnly?result:{result,committed:false,version,events:[]};}
   if(version===18446744073709551615n)throw Error('commit sequence exhausted');
   const next=version+1n;
   const events=resultOnly?null:rt.staged.map(([channel,value],i)=>({commit:next,position:BigInt(i),channel,value:eventOutput.get(channel)(value)}));
   const pending=rt.staged.map(([channel,value],i)=>({commit:next,position:BigInt(i),channel,value}));
   rt.version=next;for(const event of pending)rt.outbox.push(event);
   return resultOnly?result:{result,committed:true,version:next,events};
  }catch(e){__rollback(rt,mark);__restoreKeeps(rt);rt.version=version;rt.outbox=outbox;rt.outbox.length=outboxLength;throw e;}
  finally{rt.staged.length=0;rt.busy=false;rt.keepUndo.length=0;rt.keepSeen.clear();for(const seen of rt.seen)seen.clear();if(!rt.host)rt.undo.length=0;}
 }
 return Object.freeze({
  invoke,invoke_json:invoke,
  // A prebound typed entrypoint: scalar arguments obey the native caller contract.
  // Composite arguments still capture owned input; events remain in the outbox.
  bind(name,{resultOnly=false}={}){
   const a=actionMap.get(name);if(!a)throw Error(`unknown action ${name}`);
   const inputs=a.params.map((p,i)=>['u32','i32','u64','bool','unit','string','f32','id','enum','int'].includes(p.type.kind)?__representation(p.type,false):a.inputs[i]);
   if(!a.change&&!a.precondition&&inputs.length===0)return ()=>{
    if(rt.busy)throw Error('state operation already in progress');rt.busy=true;const ctx={fuel:100000000};
    try{const result=a.output(implementations[a.index](ctx,rt));return resultOnly?result:{result,committed:false,version:rt.version,events:[]};}finally{rt.busy=false;}
   };
   return (...args)=>run(a,args,inputs,a.output,resultOnly);
  },
  beginTransaction(){if(rt.busy||rt.host||rt.undo.length)throw Error('transaction already in progress');rt.host={version:rt.version,outbox:rt.outbox,length:rt.outbox.length};return true;},
  commitTransaction(){rt.host=null;rt.undo.length=0;},
  rollbackTransaction(){if(rt.host){__rollback(rt,0);rt.version=rt.host.version;rt.outbox=rt.host.outbox;rt.outbox.length=rt.host.length;rt.host=null;rt.keeps.fill(undefined);}},
  get version(){return rt.version;},
  get outbox(){return rt.outbox.map(e=>({...e,value:eventOutput.get(e.channel)(e.value)}));},
  acknowledgeThrough(commit,position){if(rt.busy)throw Error('state operation already in progress');commit=__stateInput(commit,{kind:'u64'},{cells:1});position=__stateInput(position,{kind:'u64'},{cells:1});rt.outbox=rt.outbox.filter(e=>e.commit>commit||(e.commit===commit&&e.position>position));},
  async invokeAsync(name,args){
   if(typeof __parallelInvokeAsync!=='function')return invoke(name,args);
   if(rt.busy)throw Error('state operation already in progress');const captured=structuredClone(args);rt.busy=true;
   try{const snapshot=await __snapshotEncode(meta,rt);const reply=await __parallelInvokeAsync(name,captured,snapshot);const s=await __snapshotDecode(meta,reply.snapshot);rt.tables=s.tables;rt.version=s.version;rt.outbox=s.outbox;rt.keeps.fill(undefined);return reply.value;}finally{rt.busy=false;}
  },
  checkpoint(limits){if(rt.busy)throw Error('checkpoint requires transaction boundary');return __snapshotEncode(meta,rt,limits);},
  async restore(bytes,limits){if(rt.host)throw Error('restore during transaction');if(rt.busy)throw Error('restore requires transaction boundary');rt.busy=true;try{const s=await __snapshotDecode(meta,bytes,limits);const candidate={...rt,tables:s.tables,version:s.version,outbox:s.outbox,keeps:rt.keeps.map(()=>undefined)};for(const name of meta.invariantChecks||[]){const a=actionMap.get(name);if(implementations[a.index]({fuel:100000000},candidate)!==true)throw Error('snapshot violates state invariant');}rt.tables=s.tables;rt.version=s.version;rt.outbox=s.outbox;rt.keeps.fill(undefined);}finally{rt.busy=false;}},
 });
}
