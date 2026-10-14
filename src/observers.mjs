// Fixed subscription lifecycle. Root/output edges derive from checked source;
// values and ownership codecs are compiled from each query's declared type.
function __createObservers(prepare){
 const subscriptions=new Map(),byRoot=new Map(),pending=new Set(),errors=[];
 let next=0,held=0,flushing=false;
 function report(s,error,phase){errors.push({query:s?.query??null,error,phase});if(s?.onError)try{s.onError(error);}catch(e){errors.push({query:s.query,error:e,phase:'error-handler'});}}
 function flush(){
  if(held||flushing||!pending.size)return;
  flushing=true;let passes=0;
  try{
   while(pending.size&&!held){
    if(++passes>1024){report(null,Error('subscription publication budget exhausted'),'publication');break;}
    const affected=[...pending];pending.clear();
    // Snapshot every affected value before consumers run. Deliveries reside in
    // the stable subscription slots; no per-delivery tuple is allocated.
    for(const id of affected){const s=subscriptions.get(id);if(!s)continue;s.deliver=false;
     try{const value=s.capture(s.read());if(!s.equal(s.value,value)){s.value=value;s.delivery=value;s.deliver=true;}}
     catch(e){report(s,e,'query');}
    }
    for(const id of affected){const s=subscriptions.get(id);if(!s||!s.deliver)continue;const value=s.delivery;s.deliver=false;s.delivery=undefined;
     try{s.callback(s.output(value));}catch(e){report(s,e,'listener');}
    }
   }
  }finally{flushing=false;}
 }
 function install(query,q,callback,{immediate=true,onError}={}){
  if(typeof callback!=='function'||onError!==undefined&&typeof onError!=='function')throw TypeError('subscription callback');
  const s={...q,id:next++,query,callback,onError,deliver:false,delivery:undefined};
  s.value=s.capture(s.read());subscriptions.set(s.id,s);
  for(const root of s.roots){let ids=byRoot.get(root);if(!ids)byRoot.set(root,ids=new Set());ids.add(s.id);}
  const dispose=()=>{if(!subscriptions.delete(s.id))return;for(const root of s.roots){const ids=byRoot.get(root);ids.delete(s.id);if(!ids.size)byRoot.delete(root);}};
  if(immediate)try{callback(s.output(s.value));}catch(e){dispose();throw e;}
  return dispose;
 }
 return {
  get active(){return subscriptions.size!==0;},
  subscribe(query,args,callback,options){return install(query,prepare(query,args),callback,options);},
  subscribeGroup(queries,callback,options){
   if(!Array.isArray(queries)||!queries.length)throw TypeError('subscription group requires queries');
   const qs=queries.map(q=>typeof q==='string'?prepare(q,[]):prepare(q.query,q.args??[]));
   const roots=[...new Set(qs.flatMap(q=>q.roots))];
   // An unknown dependency forces ordinary root invalidation for the group.
   const outputs=qs.every(q=>q.outputs?.length)?[...new Set(qs.flatMap(q=>q.outputs))]:[];
   const scalar=qs.every(q=>q.scalar),direct=qs.every(q=>q.direct);
   return install(queries,{roots,outputs,read:direct?()=>qs.map(q=>q.direct()):()=>qs.map(q=>q.read()),capture:scalar?values=>values:values=>values.map((v,i)=>qs[i].capture(v)),output:scalar?values=>values.slice():values=>values.map((v,i)=>qs[i].output(v)),equal:scalar?(a,b)=>a.every((v,i)=>v===b[i]):(a,b)=>a.every((v,i)=>qs[i].equal(v,b[i]))},callback,options);
  },
  commit(roots,outputs=null){
   if(!subscriptions.size)return;
   for(const root of roots)for(const id of byRoot.get(root)||[]){const s=subscriptions.get(id);if(outputs!==null&&s.outputs?.length&&!s.outputs.some(k=>outputs.has(k)))continue;pending.add(id);}
   flush();
  },
  hold(){held++;return new Set(pending);},
  release(){if(!held)throw Error('subscription boundary mismatch');held--;flush();},
  rollback(mark){pending.clear();for(const id of mark)pending.add(id);if(!held)throw Error('subscription boundary mismatch');held--;flush();},
  batch(callback){
   if(typeof callback!=='function'||Object.prototype.toString.call(callback)==='[object AsyncFunction]')throw TypeError('batch callback must be synchronous');held++;
   try{const value=callback();if(value&&typeof value.then==='function')throw TypeError('state batch callback must be synchronous');return value;}
   finally{held--;flush();}
  },
  flush,takeErrors(){return errors.splice(0);}
 };
}
