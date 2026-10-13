// Fixed host subscription lifecycle. Dependencies come from checked effects;
// equality and ownership codecs are compiled from each query's result type.
function __createObservers(prepare){
 const subscriptions=new Map(),byRoot=new Map(),pending=new Set(),errors=[];
 let next=0,held=0,flushing=false;
 function report(s,error,phase){
  errors.push({query:s?.query??null,error,phase});
  if(s?.onError)try{s.onError(error);}catch(e){errors.push({query:s.query,error:e,phase:'error-handler'});}
 }
 function flush(){
  if(held||flushing||!pending.size)return;
  flushing=true;let passes=0;
  try{
   while(pending.size&&!held){
    if(++passes>1024){report(null,Error('subscription publication budget exhausted'),'publication');break;}
    const affected=new Set();
    for(const root of pending)for(const id of byRoot.get(root)||[])affected.add(id);
    pending.clear();const deliveries=[];
    // Evaluate all affected queries before calling any consumers.
    for(const id of affected){
     const s=subscriptions.get(id);if(!s)continue;
     try{const value=s.capture(s.read());if(!s.equal(s.value,value)){s.value=value;deliveries.push([s,value]);}}
     catch(e){report(s,e,'query');}
    }
    for(const [s,value]of deliveries)if(subscriptions.get(s.id)===s){
     try{s.callback(s.output(value));}catch(e){report(s,e,'listener');}
    }
   }
  }finally{flushing=false;}
 }
 return {
  get active(){return subscriptions.size!==0;},
  subscribe(query,args,callback,{immediate=true,onError}={}){
   if(typeof callback!=='function'||onError!==undefined&&typeof onError!=='function')throw TypeError('subscription callback');
   const q=prepare(query,args),s={...q,id:next++,query,callback,onError};
   s.value=s.capture(s.read());subscriptions.set(s.id,s);
   for(const root of s.roots){let ids=byRoot.get(root);if(!ids)byRoot.set(root,ids=new Set());ids.add(s.id);}
   const dispose=()=>{if(!subscriptions.delete(s.id))return;for(const root of s.roots){const ids=byRoot.get(root);ids.delete(s.id);if(!ids.size)byRoot.delete(root);}};
   if(immediate)try{callback(s.output(s.value));}catch(e){dispose();throw e;}
   return dispose;
  },
  commit(roots){if(!subscriptions.size)return;for(const root of roots)pending.add(root);flush();},
  hold(){held++;return new Set(pending);},
  release(){if(!held)throw Error('subscription boundary mismatch');held--;flush();},
  rollback(mark){pending.clear();for(const root of mark)pending.add(root);if(!held)throw Error('subscription boundary mismatch');held--;flush();},
  batch(callback){
   if(typeof callback!=='function'||Object.prototype.toString.call(callback)==='[object AsyncFunction]')throw TypeError('batch callback must be synchronous');held++;
   try{const value=callback();if(value&&typeof value.then==='function')throw TypeError('state batch callback must be synchronous');return value;}
   finally{held--;flush();}
  },
  flush,
  takeErrors(){return errors.splice(0);}
 };
}
