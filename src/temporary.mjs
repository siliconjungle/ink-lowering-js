// Affine buffers expose bounded disjoint ranges over one allocation. Source
// ownership prevents retaining a parent; growth detaches shared ranges.
// Elements cannot own temporary storage: they are immutable logical values.
// Copy the slot array when acquiring ownership, never the reachable records.
class __InkBuffer {
 constructor(data,start=0,end=data.length,shared=false){this.data=data;this.start=start;this.end=end;this.shared=shared;}
 get length(){return this.end-this.start;}
 set length(n){this.end=this.start+Math.min(this.length,n);if(!this.shared&&this.start===0&&Array.isArray(this.data))this.data.length=this.end;}
 item(i){return this.data[this.start+i];}
 range(start,count){const begin=this.start+Math.min(start,this.length);return new __InkBuffer(this.data,begin,begin+Math.min(count,this.end-begin),true);}
 set(i,v){if(i<this.length){if(typeof v!=='number'&&!Array.isArray(this.data))this.detach();this.data[this.start+i]=v;}return this;}
 swap(a,b){if(a<this.length&&b<this.length){const x=this.item(a);this.set(a,this.item(b));this.set(b,x);}return this;}
 fill_range(start,count,value){start=Math.min(start,this.length);count=Math.min(count,this.length-start);if(count&&typeof value!=='number'&&!Array.isArray(this.data))this.detach();this.data.fill(value,this.start+start,this.start+start+count);return this;}
 copy_within(source,destination,count){
  const length=this.length;
  source=Math.min(source,length);destination=Math.min(destination,length);
  count=Math.min(count,length-source,length-destination);
  if(!count||source===destination)return this;
  const data=this.data,from=this.start+source,to=this.start+destination;
  if(!Array.isArray(data)){data.copyWithin(to,from,from+count);return this;}
  // Affine slot arrays are dense. Copy their values directly in overlap order.
  if(to>from&&to<from+count){
   for(let i=count-1;i>=0;i--)data[to+i]=data[from+i];
  }else{
   for(let i=0;i<count;i++)data[to+i]=data[from+i];
  }
  return this;
 }
 detach(){if(this.shared||this.start!==0||this.end!==this.data.length||!Array.isArray(this.data)){this.data=Array.isArray(this.data)?this.data.slice(this.start,this.end):Array.from(this.data.subarray(this.start,this.end));this.start=0;this.end=this.data.length;this.shared=false;}}
 push(v){if(this.length>=4294967295)throw Error('buffer length exhausted');this.detach();this.data.push(v);this.end++;return this;}
 pop(){this.length=Math.max(0,this.length-1);return this;}
 truncate(n){this.length=Math.min(this.length,n);return this;}
 slice(start=0,end=this.length){return Array.isArray(this.data)?this.data.slice(this.start+start,this.start+Math.min(end,this.length)):Array.from(this.data.subarray(this.start+start,this.start+Math.min(end,this.length)));}
 freeze(){return Array.isArray(this.data)?(this.start===0&&this.end===this.data.length?this.data:this.data.slice(this.start,this.end)):Array.from(this.data.subarray(this.start,this.end));}
 *[Symbol.iterator](){for(let i=this.start;i<this.end;i++)yield this.data[i];}
}
function __bufferSplit(x,at){const cut=x.start+Math.min(at,x.length);x.shared=true;return [new __InkBuffer(x.data,x.start,cut,true),new __InkBuffer(x.data,cut,x.end,true)];}
function __bufferAppend(a,b){
 if(a.length+b.length>4294967295)throw Error('buffer length exhausted');
 if(a.data===b.data&&a.end===b.start)return new __InkBuffer(a.data,a.start,b.end,a.start!==0||b.end!==a.data.length);
 a.detach();for(const item of b)a.push(item);return a;
}

function __repeat_until(limit,acc,stop,step){for(let i=0;i<limit;i++){if(stop(acc))break;acc=step(i,acc);}return acc;}
function __bufferFill(n,v){return new __InkBuffer(new Array(n).fill(v));}
// Normalized native F32 Numbers fit a dense private carrier. Exact NaN tags
// retain slot arrays; a later tagged write promotes only that owner's range.
function __bufferFillF32(n,v){return new __InkBuffer(typeof v==='number'?new Float32Array(n).fill(v):new Array(n).fill(v));}
function __mapGet(m,k,f){return m.has(k)?m.get(k):f();}
function __mapEntries(m,keys){return [...m.entries()].sort(([a],[b])=>a<b?-1:a>b?1:0).map(([k,v])=>keys?k:v);}
function __tempSet(x,k,v,map){x.set(k,v);return x;}
function __bufferSwap(x,a,b){return x.swap(a,b);}
function __tempEdit(x,op,...args){if(op==='push'){x.push(args[0]);}else if(op==='pop')x.pop();else if(op==='truncate')x.truncate(args[0]);else if(op==='remove')x.delete(args[0]);return x;}
