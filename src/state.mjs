// CALLER CONTRACT: arguments must match declared types; malformed data may throw or corrupt state.
function __tick(ctx){if(--ctx.fuel<0)throw Error('Ink state execution budget exhausted');}
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
function __keyCompare(a,b,type){
 if(type.kind==='string'){const x=__encoder.encode(a),y=__encoder.encode(b);for(let i=0;i<Math.min(x.length,y.length);i++)if(x[i]!==y[i])return x[i]-y[i];return x.length-y.length;}
 return a<b?-1:a>b?1:0;
}
// A table's ordered key view is reused until membership changes. Values are
// fetched at traversal time, so replacement never leaves stale row payloads.
const __tableOrder=new WeakMap();
function __orderedKeys(rt,index){const table=rt.tables[index];let keys=__tableOrder.get(table);if(!keys){keys=[...table.keys()].sort((a,b)=>__keyCompare(a,b,rt.meta.roots[index].key));__tableOrder.set(table,keys);}return keys;}
function __table(rt,index,method,args){
 const table=rt.tables[index],key=args[0];
 if(method==='values'||method==='keys'){const keys=__orderedKeys(rt,index);return method==='keys'?keys.slice():keys.map(k=>table.get(k));}
 if(method==='intersect_table'){const other=rt.tables[args[0]],small=table.size<=other.size?table:other,large=small===table?other:table;const result=[];for(const k of small.keys())if(large.has(k))result.push(k);return result.sort((a,b)=>__keyCompare(a,b,rt.meta.roots[index].key));}
 if(method==='intersect_keys'){const selected=new Set(args[0]);return __orderedKeys(rt,index).filter(k=>selected.has(k));}
 if(method==='put_many'){const keys=args[0],values=args[1];if(keys.length!==values.length)return {Err:'table bulk length mismatch'};for(let i=0;i<keys.length;i++)__table(rt,index,'put',[keys[i],values[i]]);return {Ok:null};}
 if(method==='remove_many'){for(const k of args[0])__table(rt,index,'remove',[k]);return null;}
 if(method==='get')return table.has(key)?{Some:table.get(key)}:{None:null};
 if(method==='contains')return table.has(key);
 if(method==='remove'){const present=table.has(key),value=table.get(key);if(present){__tableOrder.delete(table);rt.undo.push([index,key,true,value]);table.delete(key);}return present?{Some:value}:{None:null};}
 if(method==='insert'&&table.has(key))throw Error('insert key already present');
 if(method==='replace'&&!table.has(key))throw Error('replace key absent');
 if(!table.has(key))__tableOrder.delete(table);
 rt.undo.push([index,key,table.has(key),table.get(key)]);table.set(key,args[1]);return null;
}
function __rollback(rt,mark){for(let i=rt.undo.length-1;i>=mark;i--){const [index,key,present,value]=rt.undo[i];const table=rt.tables[index];if(table.has(key)!==present)__tableOrder.delete(table);if(present)table.set(key,value);else table.delete(key);}rt.undo.length=mark;}
function __createState(meta,implementations){
 const rt={meta,tables:meta.roots.map(()=>new Map()),version:0n,outbox:[],staged:[],busy:false,undo:[],host:null};
 const actionMap=new Map(meta.actions.map(a=>[a.name,a]));
 function invoke(name,args){
  if(rt.busy)throw Error('state operation already in progress');
  const a=actionMap.get(name);if(!a)throw Error(`unknown action ${name}`);
  const captured=a.params.map((p,i)=>__stateInput(args[i],p.type));
  const mark=rt.undo.length,version=rt.version,outbox=rt.outbox,outboxLength=rt.outbox.length;
  rt.busy=true;rt.staged=[];
  try{
   const value=implementations[a.index]({fuel:100000000},rt,...captured);
   const result=__stateOutput(value,a.result,{cells:1000000});
   if(!a.change)return {result,committed:false,version,events:[]};
   if(Object.hasOwn(value,'Err')){__rollback(rt,mark);return {result,committed:false,version,events:[]};}
   if(version===18446744073709551615n)throw Error('commit sequence exhausted');
   const next=version+1n;
   const events=rt.staged.map(([channel,value],i)=>({commit:next,position:BigInt(i),channel,value:__stateOutput(value,meta.events.find(e=>e.name===channel).type,{cells:1000000})}));
   const pending=rt.staged.map(([channel,value],i)=>({commit:next,position:BigInt(i),channel,value}));
   rt.version=next;for(const event of pending)rt.outbox.push(event);
   return {result,committed:true,version:next,events};
  }catch(e){__rollback(rt,mark);rt.version=version;rt.outbox=outbox;rt.outbox.length=outboxLength;throw e;}
  finally{rt.staged=[];rt.busy=false;if(!rt.host)rt.undo.length=0;}
 }
 return Object.freeze({
  invoke,invoke_json:invoke,
  beginTransaction(){if(rt.busy||rt.host||rt.undo.length)throw Error('transaction already in progress');rt.host={version:rt.version,outbox:rt.outbox,length:rt.outbox.length};return true;},
  commitTransaction(){rt.host=null;rt.undo.length=0;},
  rollbackTransaction(){if(rt.host){__rollback(rt,0);rt.version=rt.host.version;rt.outbox=rt.host.outbox;rt.outbox.length=rt.host.length;rt.host=null;}},
  get version(){return rt.version;},
  get outbox(){return rt.outbox.map(e=>({...e,value:__stateOutput(e.value,meta.events.find(t=>t.name===e.channel).type,{cells:1000000})}));},
  acknowledgeThrough(commit,position){if(rt.busy)throw Error('state operation already in progress');commit=__stateInput(commit,{kind:'u64'},{cells:1});position=__stateInput(position,{kind:'u64'},{cells:1});rt.outbox=rt.outbox.filter(e=>e.commit>commit||(e.commit===commit&&e.position>position));},
  async invokeAsync(name,args){
   if(typeof __parallelInvokeAsync!=='function')return invoke(name,args);
   if(rt.busy)throw Error('state operation already in progress');const captured=structuredClone(args);rt.busy=true;
   try{const snapshot=await __snapshotEncode(meta,rt);const reply=await __parallelInvokeAsync(name,captured,snapshot);const s=await __snapshotDecode(meta,reply.snapshot);rt.tables=s.tables;rt.version=s.version;rt.outbox=s.outbox;return reply.value;}finally{rt.busy=false;}
  },
  checkpoint(limits){if(rt.busy)throw Error('checkpoint requires transaction boundary');return __snapshotEncode(meta,rt,limits);},
  async restore(bytes,limits){if(rt.host)throw Error('restore during transaction');if(rt.busy)throw Error('restore requires transaction boundary');rt.busy=true;try{const s=await __snapshotDecode(meta,bytes,limits);rt.tables=s.tables;rt.version=s.version;rt.outbox=s.outbox;}finally{rt.busy=false;}},
 });
}
