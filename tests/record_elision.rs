use ink_core::{core::CheckedModule, physical, planning, syntax::parse};
use std::{fs, process::Command};
fn layout(module: &CheckedModule, name: &str) -> ink_core::record_codec::Layout {
    use ink_core::{
        logic::{self, Sort, Term},
        registry,
    };
    let schema = ink_core::record_codec::schema(module, name).unwrap();
    let var = |n: &str| Term::Var(n.into());
    let call = |f: &str, x: Term| Term::Call {
        function: f.into(),
        arguments: vec![x],
    };
    let round = |x: Term| call(&schema.decode, call(&schema.encode, x));
    let wrapped = var("wrapped");
    let payload = var("payload");
    let node = Term::Construct {
        datatype: schema.wrapper.clone(),
        constructor: 0,
        arguments: vec![payload.clone()],
    };
    let mut objects = schema.entries.clone();
    let mut roots = vec![];
    for declaration in [
        logic::Declaration::Theorem {
            params: vec![("wrapped".into(), Sort::Data(schema.wrapper.clone()))],
            conditions: vec![],
            from: round(wrapped.clone()),
            to: wrapped.clone(),
            proof: logic::Proof::Cases {
                variable: Some("wrapped".into()),
                scrutinee: None,
                from: round(wrapped.clone()),
                to: wrapped,
                cases: vec![logic::Case {
                    bindings: vec!["payload".into()],
                    proof: logic::Proof::Convert {
                        from: round(node.clone()),
                        to: node,
                    },
                }],
            },
        },
        logic::Declaration::Theorem {
            params: vec![("payload".into(), schema.payload.clone())],
            conditions: vec![],
            from: call(&schema.encode, call(&schema.decode, payload.clone())),
            to: payload.clone(),
            proof: logic::Proof::Convert {
                from: call(&schema.encode, call(&schema.decode, payload.clone())),
                to: payload,
            },
        },
    ] {
        let entry = registry::Entry::from_declaration(
            &declaration,
            schema.entries.keys().cloned().collect(),
        )
        .unwrap();
        let id = registry::identity(&entry).unwrap();
        objects.insert(id.clone(), entry);
        roots.push(id);
    }
    let roundtrip = roots[0].clone();
    let inverse = roots[1].clone();
    roots.sort();
    let ids = objects.keys().cloned().collect::<Vec<_>>();
    let bundle:registry::Bundle=serde_json::from_value(serde_json::json!({"schema":1,"snapshot":{"schema":1,"semantics":registry::SNAPSHOT_SEMANTICS,"root":registry::identity(&serde_json::json!({"schema":1,"entries":ids})).unwrap()},"roots":roots,"objects":objects,"membership":ids.iter().map(|id|(id.clone(),serde_json::json!({"entries":ids,"path":[]}))).collect::<std::collections::BTreeMap<_,_>>()})).unwrap();
    ink_core::record_codec::Layout {
        record: name.into(),
        knowledge: bundle,
        roundtrip,
        inverse,
    }
}
#[test]
fn selected_private_records_preserve_public_values_capture_transactions_and_checkpoints() {
    let source = r#"module private_records;
record Child { words:List<u32>, flag:Bool, }
record Wrap { value:Child, }
record Outer { value:Wrap, }
record Pair { first:Outer, second:Wrap, }
record Float { value:f32, }
record Exact { value:Int, }
record Proto { __proto__:String, }
enum Tree { Leaf(u32), Node(List<Tree>), }
record Packet { pair:Pair, float:Float, exact:Exact, proto:Proto, tree:Tree, }
enum Error { Failed, }
state Items:Table<u32,Packet> = Table.empty();
event changed:Tree;
change put(key:u32,packet:Packet,fail:Bool)->Result<Unit,Error> writes(Items) emits(changed) {Items.put(key,packet);emit changed(packet.tree);if fail{return Err(Error.Failed);}return Ok(());}
query get(key:u32)->Option<Packet> reads(Items) {return Items.get(key);}
query echo(packet:Packet)->Packet {return packet;}
fn identity(packet:Packet)->Packet{return packet;}
fn identity_tree(tree:Tree)->Tree{return tree;}
fn leaf(value:u32)->Tree{return Tree.Leaf(value);}
fn branch(tree:Tree)->Tree{return Tree.Node(list(tree));}
fn floats(values:List<Float>)->List<Float>{return values.map(fn(v)=>Float{value:v.value});}
fn read(value:Outer)->u32{return value.value.value.words.at_or(0,0);}
fn ignored(value:Outer)->u32{return 42;}
fn eq(a:Packet,b:Packet)->Bool{return a==b;}
"#;
    let checked = CheckedModule::from_source(parse(source).unwrap()).unwrap();
    let records = checked
        .program()
        .records
        .iter()
        .filter(|(_, fs)| fs.len() == 1)
        .map(|(n, _)| layout(&checked, n))
        .collect::<Vec<_>>();
    let mut physical:physical::Plan=serde_json::from_value(serde_json::json!({"schema":1,"semantics":physical::SEMANTICS,"input_core_sha256":checked.identity().unwrap(),"record_elisions":records})).unwrap();
    physical
        .record_elisions
        .sort_by(|a, b| a.record.cmp(&b.record));
    let plan = planning::check(
        &checked,
        &planning::Package {
            schema: 1,
            semantics: planning::SEMANTICS.into(),
            input_core_sha256: checked.identity().unwrap(),
            steps: vec![planning::Step::Physical(physical)],
        },
    )
    .unwrap();
    plan.require_record_elision_capability(false).unwrap_err();
    let selected = ink_lowering_js::lower_planned(&plan).unwrap();
    let ordinary = ink_lowering_js::lower(&checked).unwrap();
    assert!(!selected.manifest["private_record_elisions"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(selected.manifest["wire"], ordinary.manifest["wire"]);
    let dir = std::env::temp_dir().join(format!("ink-record-elision-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let extra="\nexport {__stateMetadata,__stateInputCaptures};export function captureProbe(value,cells,depth){const a=__stateMetadata.actions.find(a=>a.name==='echo'),budget={cells};try {const owned=__stateInputCaptures[a.index][0](value,budget,depth);return {value:__stateOutput(owned,a.params[0].type),cells:budget.cells};}catch(e){return {error:e.message,cells:budget.cells};}}";
    fs::write(dir.join("private.mjs"), selected.javascript + extra).unwrap();
    fs::write(dir.join("ordinary.mjs"), ordinary.javascript + extra).unwrap();
    fs::write(dir.join("test.mjs"),r#"
import assert from 'node:assert/strict';import * as ink from './private.mjs';import * as plain from './ordinary.mjs';
const tree=plain.call('branch',[plain.call('branch',[plain.call('leaf',[7])])]);
const child={words:[1,2,3],flag:true},wrapped={value:child};
const packet={pair:{first:{value:wrapped},second:wrapped},float:{value:{F32Bits:0x7fc12345}},exact:{value:{Int:'-12345678901234567890'}},proto:{['__proto__']:'own'},tree};
const p=plain.valueCodecs.identity.arguments.capture({packet}).packet;
const identity=ink.call('identity',[packet]);assert.deepEqual(identity,plain.call('identity',[packet]));assert.equal(Object.hasOwn(identity.proto,'__proto__'),true);assert.equal(Object.getPrototypeOf(identity.proto),Object.prototype);
assert.equal(ink.call('read',[packet.pair.first]),1);assert.equal(ink.call('ignored',[packet.pair.first]),42);
for(const t of [tree,plain.call('leaf',[0]),plain.call('branch',[tree])]){assert.deepEqual(ink.call('identity_tree',[t]),t);assert.deepEqual(ink.call('branch',[t]),plain.call('branch',[t]));}
for(const bits of [0x80000000,0x7fc12345,0xffc12345,0x7f800000,0x3f800000]){
 const values=[{value:{F32Bits:bits}}],a=ink.call('floats',[values]),b=plain.call('floats',[values]);assert.deepEqual(a,b);
 const destination=new Array(3);assert.equal(ink.callInto('floats',[values],destination),1);assert.deepEqual(destination[0],b[0]);assert.equal(1 in destination,false);
}
assert.equal(ink.call('eq',[packet,packet]),true);assert.equal(ink.call('eq',[packet,{...packet,proto:{['__proto__']:'other'}}]),false);
const objects=v=>{const found=new Set(),visit=x=>{if(x&&typeof x==='object'&&!found.has(x)){found.add(x);Object.values(x).forEach(visit);}};visit(v);return found;};
const owned=ink.callOwned('identity',[packet]);assert.deepEqual(owned,p);for(const x of objects(owned))assert.equal(objects(packet).has(x),false);
for(const limits of [{nodes:0},{nodes:2},{nodes:20},{depth:0},{depth:3},{depth:128},{bytes:1},{bytes:1000000}]){const outcome=fn=>{try{return fn();}catch(e){return e.message;}};assert.deepEqual(outcome(()=>ink.callOwned('identity',[packet],limits)),outcome(()=>plain.callOwned('identity',[packet],limits)));}
for(const compact of [false,true]){const args=ink.valueCodecs.identity.arguments,result=ink.valueCodecs.identity.result,request=(compact?args.compact():args).encode({packet}),reply=(compact?ink.callCompact:ink.callBinary)('identity',request);assert.deepEqual(reply,(compact?plain.callCompact:plain.callBinary)('identity',request));assert.deepEqual((compact?result.compact():result).decode(reply),p);}
const invalid=[packet,null,{...packet,proto:{}},{...packet,pair:{first:{value:wrapped}}},{...packet,float:{value:'x'}},{...packet,tree:{payload:{Ok:4}}}];
for(const value of invalid)for(const cells of [0,1,2,10,50,1000000])for(const depth of [0,63,64,65])assert.deepEqual(ink.captureProbe(value,cells,depth),plain.captureProbe(value,cells,depth));
for(const cells of [1,2,10,50,1000000]){const trace=mod=>{let reads=[];const host={};for(const key of Object.keys(packet))Object.defineProperty(host,key,{enumerable:true,get(){reads.push(key);return packet[key];}});return {reads,result:mod.captureProbe(host,cells,0)};};assert.deepEqual(trace(ink),trace(plain));}
const state=ink.createState(),control=plain.createState();
for(const [key,value,fail]of [[0,packet,false],[1,packet,true],[0,{...packet,proto:{['__proto__']:'new'}},true],[2,packet,false]]){
 assert.deepEqual(state.invoke_json('put',[key,value,fail]),control.invoke_json('put',[key,value,fail]));
 assert.deepEqual(state.invoke_json('get',[key]),control.invoke_json('get',[key]));
 assert.deepEqual(await state.checkpoint(),await control.checkpoint());
}
const first=state.invoke_json('get',[0]).result.Some;first.pair.first.value.value.words[0]=999;assert.equal(state.invoke_json('get',[0]).result.Some.pair.second.value.words[0],1);
const prepared=state.prepare('echo',[packet]);assert.deepEqual(state.invokePrepared(prepared),control.invokePrepared(control.prepare('echo',[packet])));
const ordinaryBytes=await control.checkpoint(),privateBytes=await state.checkpoint();assert.deepEqual(ordinaryBytes,privateBytes);
const restored=ink.createState(),restoredPlain=plain.createState();await restored.restore(ordinaryBytes);await restoredPlain.restore(privateBytes);assert.deepEqual(restored.invoke_json('get',[0]),restoredPlain.invoke_json('get',[0]));assert.deepEqual(await restored.checkpoint(),ordinaryBytes);
for(const limits of [{values:0},{values:2},{depth:0},{depth:2},{bytes:200}]){const outcome=async fn=>{try{return await fn();}catch(e){return e.message;}};assert.deepEqual(await outcome(()=>state.checkpoint(limits)),await outcome(()=>control.checkpoint(limits)));assert.deepEqual(await outcome(()=>restored.restore(ordinaryBytes,limits)),await outcome(()=>restoredPlain.restore(ordinaryBytes,limits)));}
console.log('private record conformance passed');
"#).unwrap();
    let result = Command::new("node")
        .arg(dir.join("test.mjs"))
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    fs::remove_dir_all(dir).unwrap();
}
#[test]
fn private_records_work_in_native_float_pure_modules() {
    let checked=CheckedModule::from_source(parse("module private_float;record Float{value:f32,} fn copy(x:Float)->Float{return Float{value:x.value};} fn add(x:Float)->Float{return Float{value:x.value+x.value};} fn many(xs:List<Float>)->List<Float>{return xs.map(fn(x)=>Float{value:x.value});}").unwrap()).unwrap();
    let physical:physical::Plan=serde_json::from_value(serde_json::json!({"schema":1,"semantics":physical::SEMANTICS,"input_core_sha256":checked.identity().unwrap(),"record_elisions":[layout(&checked,"Float")]})).unwrap();
    let plan = planning::check(
        &checked,
        &planning::Package {
            schema: 1,
            semantics: planning::SEMANTICS.into(),
            input_core_sha256: checked.identity().unwrap(),
            steps: vec![planning::Step::Physical(physical)],
        },
    )
    .unwrap();
    let dir = std::env::temp_dir().join(format!("ink-record-elision-float-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("private.mjs"),
        ink_lowering_js::lower_planned(&plan).unwrap().javascript,
    )
    .unwrap();
    fs::write(
        dir.join("ordinary.mjs"),
        ink_lowering_js::lower(&checked).unwrap().javascript,
    )
    .unwrap();
    fs::write(dir.join("test.mjs"),r#"import assert from 'node:assert/strict';import * as ink from './private.mjs';import * as plain from './ordinary.mjs';
 for(const bits of [0,0x80000000,0x7f800000,0xff800000,0x7fc12345,0xffc12345,0x3fc00000]){const value={value:{F32Bits:bits}};for(const name of ['copy','add']){assert.deepEqual(ink.call(name,[value]),plain.call(name,[value]));assert.deepEqual(ink.callOwned(name,[value]),plain.callOwned(name,[value]));}const destination=new Array(2);assert.equal(ink.callInto('many',[[value]],destination),1);assert.deepEqual(destination[0],plain.call('copy',[value]));for(const compact of [false,true]){const request=(compact?ink.valueCodecs.copy.arguments.compact():ink.valueCodecs.copy.arguments).encode({x:value});assert.deepEqual((compact?ink.callCompact:ink.callBinary)('copy',request),(compact?plain.callCompact:plain.callBinary)('copy',request));}}
 "#).unwrap();
    let result = Command::new("node")
        .arg(dir.join("test.mjs"))
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    fs::remove_dir_all(dir).unwrap();
}
