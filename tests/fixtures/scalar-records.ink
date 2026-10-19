module ScalarRecords;
record Pair { a:f32,b:f32, }
record Bundle { p:Pair,q:Pair,r:Pair,s:Pair,t:Pair,u:Pair, }
fn component(w:Pair,x:f32)->f32 {let a=w.a-x;let b=w.b+x;return a*a+b*b;}
fn energy(w:Bundle,x:f32)->f32 {
    let a=w.p.a-x;let b=w.p.b+x;let c=w.q.a-x;let d=w.q.b+x;
    let e=w.r.a-x;let f=w.r.b+x;let g=w.s.a-x;let h=w.s.b+x;
    let i=w.t.a-x;let j=w.t.b+x;let k=w.u.a-x;let l=w.u.b+x;
    return a*a+b*b+c*c+d*d+e*e+f*f+g*g+h*h+i*i+j*j+k*k+l*l;
}
fn derivatives(w:Bundle,x:f32)->Bundle {return gradient(energy,w,x);}
fn direction(w:Bundle,x:f32)->f32 {return jvp(energy,w,w,x);}
fn twice(w:Pair)->f32 {return w.a*w.a+w.a*w.a;}
fn selected(w:Pair,c:Bool)->Pair {return choose(c,Pair{a:w.b*w.b,b:w.a*w.a},Pair{a:w.a*w.a,b:w.b*w.b});}
fn branches(x:f32,c:Bool)->Pair {let r=Pair{a:x,b:-x};return selected(r,c);}
fn looping(x:f32)->Pair {return repeat(3,Pair{a:x,b:-x},fn(i)=>fn(r)=>Pair{a:r.b,b:r.a});}
fn strict_unused(w:Pair)->f32 {let ignored=w.b*w.b;return w.a;}
fn strict(x:f32,y:f32)->f32 {return strict_unused(Pair{a:x,b:y});}
record Node { value:u32,previous:Option<Node>, }
fn history(x:u32)->Node {return repeat(3,Node{value:x,previous:None},fn(i)=>fn(n)=>Node{value:n.value+1,previous:Some(n)});}
