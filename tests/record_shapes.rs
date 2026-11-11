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
    assert_eq!(shapes.matches("return (value,context)=>").count(), 4, "{shapes}");
    assert!(shapes.contains(r#"["__proto__"]:c0(value["__proto__"],context)"#), "{shapes}");
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
assert.equal(compared,inputs.length);
"#,
    )
    .unwrap();
    let output = Command::new("node").arg(root.join("test.mjs")).output().unwrap();
    fs::remove_dir_all(root).unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}
