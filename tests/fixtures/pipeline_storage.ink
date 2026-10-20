module pipeline_storage;
record Work{items:List<f32>,amount:f32,}
fn advance(xs:List<f32>,amount:f32)->List<f32>{return xs.map(fn(x)=>x+amount);}
fn relay(xs:List<f32>,amount:f32)->List<f32>{return advance(xs,amount);}
fn stages(xs:List<f32>,amount:f32)->List<f32>{return relay(xs,amount).map_indexed(fn(i)=>fn(x)=>x*0.5).filter(fn(x)=>x>0.0).map(fn(x)=>x-amount);}
fn wrapped(xs:List<f32>,amount:f32)->List<f32>{let work=Work{items:relay(xs,amount),amount:amount};return work.items.map(fn(x)=>x*work.amount).filter(fn(x)=>x>0.0);}
fn indexed(keys:List<u32>,xs:List<f32>,amount:f32)->List<f32>{let table:Map<u32,Bool>=foldr(keys,Map.empty(),fn(k)=>fn(m)=>m.set(k,true));return xs.map_indexed(fn(i)=>fn(x)=>choose(table.contains(i),x+amount,x));}
record Pair{left:List<f32>,right:List<f32>,}
fn identity(xs:List<f32>)->List<f32>{return xs;}
fn borrowed(xs:List<f32>,amount:f32)->List<f32>{return identity(xs).map(fn(x)=>x+amount);}
fn shared(xs:List<f32>,amount:f32)->Pair{let a=relay(xs,amount);return Pair{left:a,right:a.map(fn(x)=>x+amount)};}
fn captured(xs:List<f32>,amount:f32)->List<f32>{let a=relay(xs,amount);return a.map(fn(x)=>x+sum(a));}
fn pending(xs:List<f32>,amount:f32)->List<f32>{let a=relay(xs,amount);return a.zip(a.map(fn(x)=>x+amount),fn(x)=>fn(y)=>x+y);}
fn selected(xs:List<f32>,amount:f32,flag:Bool)->List<f32>{return choose(flag,stages(xs,amount),borrowed(xs,amount));}
fn order(xs:List<u32>)->List<u32>{return xs.map(fn(x)=>x+1).map(fn(x)=>x*3);}
fn cached_rows(keys:List<u32>,rows:List<Work>)->List<Work>{let table:Map<u32,Bool>=foldr(keys,Map.empty(),fn(k)=>fn(m)=>m.set(k,true));return rows.map_indexed(fn(i)=>fn(row)=>choose(table.contains(i),row,Work{items:row.items,amount:-row.amount}));}
fn halve(xs:List<f32>)->List<f32>{return xs.map(fn(x)=>x*0.5);}
fn positive(xs:List<f32>)->List<f32>{return xs.filter(fn(x)=>x>0.0);}
fn helpers(xs:List<f32>,amount:f32)->List<f32>{return positive(halve(advance(xs,amount)));}
fn helper_alias(xs:List<f32>,amount:f32)->Pair{let a=advance(xs,amount);return Pair{left:a,right:halve(a)};}
fn helper_pending(xs:List<f32>,amount:f32)->List<f32>{let a=advance(xs,amount);return a.zip(halve(a),fn(x)=>fn(y)=>x+y);}
