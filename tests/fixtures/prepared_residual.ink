module Residual;
fn indexed(keys:List<u32>,queries:List<u32>)->List<Bool>{
 let index:Map<u32,Bool>=foldr(keys,Map.empty(),fn(k)=>fn(m)=>m.set(k,true));
 return queries.map(fn(q)=>index.contains(q));}
fn guarded(keys:List<u32>,queries:List<u32>)->List<Bool>{
 if length(queries)>=2 {
 let index:Map<u32,Bool>=foldr(keys,Map.empty(),fn(k)=>fn(m)=>m.set(k,true));
 return queries.map(fn(q)=>index.contains(q));}
 return queries.map(fn(q)=>foldr(keys,false,fn(k)=>fn(found)=>k==q || found));}
fn edited(keys:List<u32>,queries:List<u32>)->List<Bool>{
 let index:Map<u32,Bool>=foldr(keys,Map.empty(),fn(k)=>fn(m)=>m.set(k,true));
 let changed=index.set(queries.at_or(0,0),true);
 return queries.map(fn(q)=>changed.contains(q));}
fn transformed(xs:List<u32>,scale:u32)->List<u32>{let base=xs.map(fn(x)=>x+1);let values=base.map(fn(x)=>x*scale);return values.concat([scale]);}
fn shadowed(xs:List<u32>,ys:List<u32>)->List<u32>{let xs=xs.map(fn(x)=>x+1);let saved=xs;let xs=ys.map(fn(y)=>y*3);return saved.concat(xs);}
fn floating(xs:List<f32>,negate:Bool)->List<f32>{let base=xs.map(fn(x)=>-x);return choose(negate,base,xs);}
spec fn checked(keys:List<u32>,queries:List<u32>,allowed:Bool)->List<Bool> requires allowed; ensures true;
implementation check_body for checked{let index:Map<u32,Bool>=foldr(keys,Map.empty(),fn(k)=>fn(m)=>m.set(k,true));return queries.map(fn(q)=>index.contains(q));}
fn plain(value:u32)->u32{return value*2;}
