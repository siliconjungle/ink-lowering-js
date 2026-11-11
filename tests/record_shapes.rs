use ink_core::{core::CheckedModule, syntax::parse};
use std::{fs, process::Command};

/// Literal record converters are an optimisation of the generic field walk.
/// The same emitted module is loaded with them and with the table emptied, and
/// every observable of a public call has to agree: values, own property
/// descriptors and their order, prototypes, the order host fields are read in,
/// sharing between repeated inputs, and failures.
#[test]
fn literal_record_converters_match_the_generic_field_walk() {
    let source = r#"module record_shapes;
record Point { x:f32, y:f32, }
record Offset { x:f32, y:f32, }
record Proto { __proto__:f32, plain:f32, }
record Line { from:Point, to:Point, label:String, weight:f64, }
record Bag { points:List<Point>, first:Option<Point>, exact:Int, proto:Proto, }
fn point(p:Point)->Point{return p;}
fn offset(p:Offset)->Offset{return Offset{x:p.x+1.0,y:p.y};}
fn proto(p:Proto)->Proto{return p;}
fn line(l:Line)->Line{return Line{from:l.to,to:l.from,label:l.label,weight:l.weight};}
fn bag(b:Bag)->Bag{return b;}
fn points(ps:List<Point>)->List<Point>{return ps.map(fn(p)=>Point{x:p.y,y:p.x});}
fn pair(a:Point,b:Point)->List<Point>{return list(a,b);}
"#;
    let checked = CheckedModule::from_source(parse(source).unwrap()).unwrap();
    let emitted = ink_lowering_js::lower(&checked).unwrap().javascript;
    // One literal per distinct field list: Point and Offset share theirs.
    let start = emitted
        .find("const __recordShapes=new Map([")
        .expect("record shapes are emitted with the representation helper");
    let shapes = &emitted[start..];
    let shapes = &shapes[..shapes.find("]);\n").unwrap() + 4];
    assert_eq!(shapes.matches("(c,result)=>").count(), 4, "{shapes}");
    assert!(shapes.contains(r#"["__proto__"]:c0(value["__proto__"],context)"#), "{shapes}");
    assert!(shapes.contains(r#"{["__proto__"]:r0,"plain":r1}"#), "{shapes}");
    let generic = emitted.replacen(shapes, "const __recordShapes=new Map();\n", 1);
    assert_ne!(generic, emitted);
    let root = std::env::temp_dir().join(format!("ink-record-shapes-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("literal.mjs"), &emitted).unwrap();
    fs::write(root.join("generic.mjs"), &generic).unwrap();
    fs::write(
        root.join("test.mjs"),
        r#"
import assert from 'node:assert/strict';
import * as literal from './literal.mjs';
import * as generic from './generic.mjs';
const nan={F32Bits:0x7fc12345},proto=(a,b)=>{const p={};Object.defineProperty(p,'__proto__',{value:a,writable:true,enumerable:true,configurable:true});p.plain=b;return p;};
const point=(x,y)=>({x,y}),shared=point(1.5,-0);
const inputs=[
 ['point',()=>[point(0.1,nan)]],
 ['offset',()=>[point(-0,3)]],
 ['proto',()=>[proto(2.5,nan)]],
 ['line',()=>[{from:point(1,2),to:point(3,nan),label:'雪',weight:{F64Bits:'0x7ff8000000000001'}}]],
 ['line',()=>{const p=point(4,5);return [{from:p,to:p,label:'',weight:-0}];}],
 ['bag',()=>[{points:[shared,point(2,3),shared],first:{Some:shared},exact:{Int:'-12345678901234567890'},proto:proto(nan,1)}]],
 ['bag',()=>[{points:[],first:{None:null},exact:{Int:'0'},proto:proto(0,0)}]],
 ['points',()=>[[shared,shared,point(nan,7)]]],
 ['pair',()=>[shared,shared]],
 // Failures: a missing record, a missing field, a wrong field type.
 ['point',()=>[null]],
 ['line',()=>[{from:point(1,2),label:'x',weight:1}]],
 ['bag',()=>[{points:[point(1,2),7],first:{None:null},exact:{Int:'0'},proto:proto(0,0)}]],
];
// A structural description that keeps what deepEqual ignores: property order,
// descriptors, prototypes, negative zero and which positions share one object.
function describe(value){
 const seen=new Map();
 const visit=v=>{
  if(v===null||typeof v!=='object')return Object.is(v,-0)?'-0':typeof v==='bigint'?v+'n':v;
  if(seen.has(v))return {same:seen.get(v)};
  seen.set(v,seen.size);
  const proto=Object.getPrototypeOf(v);
  return {proto:proto===Object.prototype?'Object':proto===Array.prototype?'Array':proto===null?'null':'other',
   own:Reflect.ownKeys(v).map(k=>{const d=Object.getOwnPropertyDescriptor(v,k);return [String(k),d.writable,d.enumerable,d.configurable,visit(d.value)];})};
 };
 return visit(value);
}
// Host objects whose every field read is recorded, to compare read order.
function traced(value,reads,path='arg'){
 if(value===null||typeof value!=='object'||Array.isArray(value))return Array.isArray(value)?value.map((v,i)=>traced(v,reads,path+'['+i+']')):value;
 if(Object.hasOwn(value,'F32Bits')||Object.hasOwn(value,'F64Bits')||Object.hasOwn(value,'Int')||Object.hasOwn(value,'Some')||Object.hasOwn(value,'None'))return value;
 const host={};
 for(const key of Reflect.ownKeys(value)){const inner=traced(value[key],reads,path+'.'+String(key));Object.defineProperty(host,key,{enumerable:true,configurable:true,get(){reads.push(path+'.'+String(key));return inner;}});}
 return host;
}
const outcome=(module,name,args)=>{try{return {value:describe(module.call(name,args))};}catch(error){return {error:error.constructor.name+': '+error.message};}};
let compared=0;
for(const [name,make] of inputs){
 assert.deepStrictEqual(outcome(literal,name,make()),outcome(generic,name,make()),name);
 const a=[],b=[];
 assert.deepStrictEqual(outcome(literal,name,make().map((v,i)=>traced(v,a,'arg'+i))),outcome(generic,name,make().map((v,i)=>traced(v,b,'arg'+i))),name+' traced');
 assert.deepStrictEqual(a,b,name+' read order');
 if(name==='point'&&make()[0])assert.ok(a.length>0);
 compared++;
}
// The cases above exercise what they claim to.
const bag=literal.call('bag',[inputs[5][1]()[0]]);
assert.strictEqual(bag.points[0],bag.points[2]);assert.strictEqual(bag.points[0],bag.first.Some);
assert.strictEqual(Object.getPrototypeOf(bag.proto),Object.prototype);assert.ok(Object.hasOwn(bag.proto,'__proto__'));
assert.deepStrictEqual(Reflect.ownKeys(bag),['points','first','exact','proto']);assert.deepStrictEqual(Reflect.ownKeys(bag.proto),['__proto__','plain']);
assert.ok(outcome(literal,'point',[null]).error);
// A non-finite field still leaves as its bit carrier, and a field that is not
// a binary32 value is rounded, with or without the literals.
for(const module of [literal,generic]){
 assert.deepStrictEqual(module.call('point',[point(Infinity,1)]),{x:{F32Bits:0x7f800000},y:1});
 assert.strictEqual(module.call('point',[point(0.1,1)]).x,Math.fround(0.1));
 assert.deepStrictEqual(module.call('points',[[point(nan,1),point(2,3)]]),[{x:1,y:{F32Bits:0x7fc12345}},{x:3,y:2}]);
}
assert.equal(compared,inputs.length);
"#,
    )
    .unwrap();
    let output = Command::new("node").arg(root.join("test.mjs")).output().unwrap();
    fs::remove_dir_all(root).unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}

/// A result is converted only where it needs it. The converters are driven
/// directly here, because through a module an argument that reaches the result
/// is copied on the way in and the kernel's own objects cannot be told apart.
#[test]
fn results_keep_what_needs_no_conversion_and_share_what_is_rebuilt() {
    let helper = include_str!("../src/values.mjs").split("// @helper state-validation").next().unwrap();
    let script = r#"
const assert=require('node:assert/strict');
const __namedTypes={},__nativeF32=true;
const f32={kind:'f32'},point={kind:'record',name:'Point',fields:[{name:'x',type:f32},{name:'y',type:f32}]};
const line={kind:'record',name:'Line',fields:[{name:'from',type:point},{name:'to',type:{kind:'option',element:point}},{name:'n',type:{kind:'int'}},{name:'__proto__',type:f32}]};
const literalShapes=new Map([['["x","y"]',(c,result)=>{const c0=c[0],c1=c[1];return result?(value,context)=>{const v0=value["x"],r0=c0(v0,context),v1=value["y"],r1=c1(v1,context);return r0===v0&&r1===v1?value:{"x":r0,"y":r1};}:(value,context)=>({"x":c0(value["x"],context),"y":c1(value["y"],context)});}]]);
for(const __recordShapes of [undefined,literalShapes]){
 globalThis.__recordShapes=__recordShapes;
 const result=type=>__representation(type,true,false,true);
 const points=result({kind:'list',element:point}),floats=result({kind:'list',element:f32}),one=result(point),whole=result(line),some=result({kind:'option',element:point});
 const context=()=>new WeakMap();
 // Finite binary32 values: the very objects come back and nothing is remembered.
 const a={x:0.5,y:-0},b={x:2,y:3},list=[a,b,a],c=context();
 assert.strictEqual(points(list,c),list);assert.strictEqual(one(a,c),a);assert.equal(c.rebuilt,undefined);
 const numbers=[0.5,-0,3.4028234663852886e38];assert.strictEqual(floats(numbers,c),numbers);
 const none={None:null},someA={Some:a};assert.strictEqual(some(none,c),none);assert.strictEqual(some(someA,c),someA);assert.equal(c.rebuilt,undefined);
 // One element that needs a carrier: only it is rebuilt, the list is new, the rest are themselves.
 const inf={x:Infinity,y:1},mixed=[a,inf,b,inf,a],d=context(),out=points(mixed,d);
 assert.notStrictEqual(out,mixed);assert.strictEqual(out[0],a);assert.strictEqual(out[2],b);assert.strictEqual(out[4],a);
 assert.deepStrictEqual(out[1],{x:{F32Bits:0x7f800000},y:1});assert.strictEqual(out[1],out[3]);assert.equal(d.rebuilt,true);
 assert.deepStrictEqual(mixed,[a,inf,b,inf,a]);assert.strictEqual(inf.x,Infinity);
 // After the first rebuild every object of the call is remembered, so later repeats stay shared too.
 const late={x:NaN,y:0};assert.strictEqual(one(late,d),one(late,d));assert.strictEqual(one(a,d),a);
 // A value that is not a binary32 Number is rounded; -0 and the largest finite value are kept.
 assert.deepStrictEqual(floats([0.1,1],context()),[Math.fround(0.1),1]);
 assert.deepStrictEqual(floats([1,NaN,-Infinity],context()),[1,{F32Bits:0x7fc00000},{F32Bits:0xff800000}]);
 assert.ok(Object.is(floats([-0],context())[0],-0));
 // Other carriers of a list are still copied into an Array.
 const typed=new Float32Array([1,2]),copied=floats(typed,context());assert.ok(Array.isArray(copied));assert.deepStrictEqual(copied,[1,2]);
 // Nested: an Int always needs its carrier, so the outer record is rebuilt around kept inner records,
 // and __proto__ stays an own data property.
 const inner={x:1,y:2},host={from:inner,to:{Some:inner},n:12345678901234567890n};
 Object.defineProperty(host,'__proto__',{value:1.5,writable:true,enumerable:true,configurable:true});
 const e=context(),rebuilt=whole(host,e);
 assert.notStrictEqual(rebuilt,host);assert.strictEqual(rebuilt.from,inner);assert.strictEqual(rebuilt.to,host.to);
 assert.deepStrictEqual(rebuilt.n,{Int:'12345678901234567890'});assert.deepStrictEqual(Reflect.ownKeys(rebuilt),['from','to','n','__proto__']);
 assert.deepStrictEqual(Object.getOwnPropertyDescriptor(rebuilt,'__proto__'),{value:1.5,writable:true,enumerable:true,configurable:true});
 assert.strictEqual(Object.getPrototypeOf(rebuilt),Object.prototype);
 // Destinations and state results use the plain flavour: always a copy.
 const copying=__representation(point,true),g=context(),written=copying(a,g);assert.notStrictEqual(written,a);assert.deepStrictEqual(written,a);
 // Arguments are still always rebuilt and remembered.
 const input=__representation(point,false,true),f=context(),copy=input(a,f);
 assert.notStrictEqual(copy,a);assert.strictEqual(input(a,f),copy);assert.deepStrictEqual(copy,a);
}
"#;
    let dir = std::env::temp_dir().join(format!("ink-result-keep-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("test.cjs"), format!("{helper}\n{script}")).unwrap();
    let output = Command::new("node").arg(dir.join("test.cjs")).output().unwrap();
    fs::remove_dir_all(dir).unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}
