// Fixed state primitives used by emitted action bodies, without an AST evaluator.
class __Abort {constructor(value){this.value=value;}}
function __try(value){if(Object.hasOwn(value,'Err'))throw new __Abort(value.Err);return value.Ok;}
function __nested(value,change){if(change && Object.hasOwn(value,'Err'))throw new __Abort(value.Err);return value;}
function __equal(a,b){if(a===b)return true;if(!a||!b||typeof a!=='object'||typeof b!=='object')return false;const ak=Object.keys(a),bk=Object.keys(b);return ak.length===bk.length&&ak.every(k=>Object.hasOwn(b,k)&&__equal(a[k],b[k]));}
function __checked(op,a,b,bits){const x=BigInt(a),y=BigInt(b),n=op==='checked_add'?x+y:op==='checked_sub'?x-y:x*y;return n<0n||n>=(1n<<BigInt(bits))?{Err:'ArithmeticError.Overflow'}:{Ok:bits===32?Number(n):n};}
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
function __table(rt,index,method,args){
 const table=rt.tables[index],key=args[0];
 if(method==='values')return [...table.entries()].sort((a,b)=>__keyCompare(a[0],b[0],rt.meta.roots[index].key)).map(([k,v])=>v);
 if(method==='get')return table.has(key)?{Some:table.get(key)}:{None:null};
 if(method==='contains')return table.has(key);
 if(method==='remove'){const old=table.has(key)?{Some:table.get(key)}:{None:null};table.delete(key);return old;}
 if(method==='insert'&&table.has(key))throw Error('insert key already present');
 if(method==='replace'&&!table.has(key))throw Error('replace key absent');
 table.set(key,args[1]);return null;
}
function __createState(meta,implementations){
 const rt={meta,tables:meta.roots.map(()=>new Map()),version:0n,outbox:[],staged:[],busy:false};
 const actionMap=new Map(meta.actions.map(a=>[a.name,a]));
 function invoke(name,args){
  if(rt.busy)throw Error('state operation already in progress');
  const a=actionMap.get(name);if(!a)throw Error(`unknown action ${name}`);
  if(!Array.isArray(args)||args.length!==a.params.length)__fail('argument count mismatch');
  const captured=args.map((v,i)=>__in(v,a.params[i].type,{cells:1000000}));
  const tables=rt.tables.map(t=>new Map(t)),version=rt.version,outbox=rt.outbox;
  rt.busy=true;rt.staged=[];
  try{
   const value=implementations[a.index]({fuel:100000000},rt,...captured);
   const result=__out(value,a.result,{cells:1000000});
   if(!a.change)return {result,committed:false,version,events:[]};
   if(Object.hasOwn(value,'Err')){rt.tables=tables;return {result,committed:false,version,events:[]};}
   if(version===18446744073709551615n)throw Error('commit sequence exhausted');
   const next=version+1n;
   const events=rt.staged.map(([channel,value],i)=>({commit:next,position:BigInt(i),channel,value:__out(value,meta.events.find(e=>e.name===channel).type,{cells:1000000})}));
   const pending=rt.staged.map(([channel,value],i)=>({commit:next,position:BigInt(i),channel,value}));
   rt.version=next;rt.outbox=outbox.concat(pending);
   return {result,committed:true,version:next,events};
  }catch(e){rt.tables=tables;rt.version=version;rt.outbox=outbox;throw e;}
  finally{rt.staged=[];rt.busy=false;}
 }
 return Object.freeze({
  invoke,invoke_json:invoke,
  get version(){return rt.version;},
  get outbox(){return rt.outbox.map(e=>({...e,value:__out(e.value,meta.events.find(t=>t.name===e.channel).type,{cells:1000000})}));},
  acknowledgeThrough(commit,position){if(rt.busy)throw Error('state operation already in progress');commit=__in(commit,{kind:'u64'},{cells:1});position=__in(position,{kind:'u64'},{cells:1});rt.outbox=rt.outbox.filter(e=>e.commit>commit||(e.commit===commit&&e.position>position));},
  checkpoint(limits){if(rt.busy)throw Error('checkpoint requires transaction boundary');return __snapshotEncode(meta,rt,limits);},
  async restore(bytes,limits){if(rt.busy)throw Error('restore requires transaction boundary');rt.busy=true;try{const s=await __snapshotDecode(meta,bytes,limits);rt.tables=s.tables;rt.version=s.version;rt.outbox=s.outbox;}finally{rt.busy=false;}},
 });
}
