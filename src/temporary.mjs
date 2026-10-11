
function __repeat_until(limit,acc,stop,step){for(let i=0;i<limit;i++){if(stop(acc))break;acc=step(i,acc);}return acc;}
function __bufferFill(n,v){return Array.from({length:n},()=>__tempCopy(v));}
function __tempCopy(v){if(Array.isArray(v)||(ArrayBuffer.isView(v)&&typeof v.length==='number'))return Array.from(v,__tempCopy);if(v&&typeof v==='object')return Object.fromEntries(Object.entries(v).map(([k,v])=>[k,__tempCopy(v)]));return v;}
function __mapGet(m,k,f){return m.has(k)?__tempCopy(m.get(k)):f();}
function __mapEntries(m,keys){return [...m.entries()].sort(([a],[b])=>a<b?-1:a>b?1:0).map(([k,v])=>__tempCopy(keys?k:v));}
function __tempSet(x,k,v,map){if(map)x.set(k,__tempCopy(v));else if(k<x.length)x[k]=__tempCopy(v);return x;}
function __bufferSwap(x,a,b){if(a<x.length&&b<x.length){const v=x[a];x[a]=x[b];x[b]=v;}return x;}
function __tempEdit(x,op,...args){if(op==='push'){x.push(__tempCopy(args[0]));}else if(op==='pop')x.pop();else if(op==='truncate')x.length=Math.min(x.length,args[0]);else if(op==='remove')x.delete(args[0]);return x;}
