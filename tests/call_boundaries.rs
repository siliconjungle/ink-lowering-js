use ink_core::{core::CheckedModule, eval, syntax::parse};
use serde_json::json;
use std::{fs, process::Command};

#[test]
fn fixed_boundaries_and_prepared_destinations_preserve_exact_values_and_private_ownership() {
    let base = r#"module boundaries;
record Row{value:f32,items:List<f32>,}
fn scalar(x:f32,y:f32)->f32{return x*y+x;}
fn word_float(x:u32)->f32{return f32_from_u32(x);}
fn pair(x:f32,y:f32)->List<f32>{return list(x,y);}
fn shift(xs:List<f32>,amount:f32)->List<f32>{return xs.map(fn(x)=>x+amount);}
fn rows(xs:List<Row>)->List<Row>{return xs.map(fn(x)=>x);}
fn nested(xs:List<List<f32>>)->List<List<f32>>{return xs.map(fn(x)=>x);}
fn indexed(keys:List<u32>,xs:List<u32>)->List<Bool>{let table:Map<u32,Bool>=foldr(keys,Map.empty(),fn(k)=>fn(m)=>m.set(k,true));return xs.map(fn(x)=>table.contains(x));}
spec fn checked(xs:List<f32>,allowed:Bool)->List<f32> requires allowed;ensures true;
implementation checked_body for checked{return xs;}
"#;
    let dir = std::env::temp_dir().join(format!("ink-call-boundaries-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    for mode in ["native", "captured", "bits", "wide"] {
        let mut source = base.to_owned();
        if mode == "wide" {
            source = source.replace("f32", "f64");
            let at = source.find("spec fn checked").unwrap();
            source.truncate(at);
        }
        if matches!(mode, "captured" | "bits") {
            source += "state Counter:u32=0;";
        }
        if mode == "bits" {
            source += "fn owner(x:Buffer<f32>)->Buffer<f32>{return x;}";
        }
        let module = CheckedModule::from_source(parse(&source).unwrap()).unwrap();
        let mut cases = vec![];
        let words: Vec<u64> = if mode == "wide" {
            vec![
                0,
                1,
                1 << 63,
                0x3ff0000000000001,
                0x7ff0000000000000,
                0x7ff0000000000001,
                0xfff8000000005678,
            ]
        } else {
            vec![
                0, 1, 0x80000000, 0x3f800001, 0x7f800000, 0x7f800001, 0xffc12345,
            ]
        };
        let scalar = |v: u64| {
            if mode == "wide" {
                json!({"F64Bits":v.to_string()})
            } else {
                json!({"F32Bits":v})
            }
        };
        for (i, word) in words.iter().enumerate() {
            let x = scalar(*word);
            let y = scalar(words[(i + 3) % words.len()]);
            for (name, args) in [
                ("scalar", json!([x, y])),
                ("pair", json!([x, y])),
                ("shift", json!([[x, y], x])),
                (
                    "rows",
                    json!([[{"value":x,"items":[x,y]},{"value":y,"items":[]}]]),
                ),
                ("nested", json!([[[x, y], [], [y, x]]])),
                ("indexed", json!([[1, 2, 7], [i, 7, 99]])),
                ("checked", json!([[x, y], true])),
            ] {
                if mode == "wide" && name == "checked" {
                    continue;
                }
                let f = module
                    .program()
                    .functions
                    .iter()
                    .find(|f| f.name == name)
                    .unwrap();
                let values = args
                    .as_array()
                    .unwrap()
                    .iter()
                    .zip(&f.params)
                    .map(|(v, (_, ty))| {
                        eval::Value::from_program_json(v, ty, module.program()).unwrap()
                    })
                    .collect();
                let expected = eval::call_checked(&module, name, values, &mut 1_000_000)
                    .unwrap()
                    .json();
                cases.push(json!({"name":name,"args":args,"expected":expected}));
            }
        }
        let emitted = ink_lowering_js::lower(&module).unwrap();
        fs::write(dir.join("program.mjs"), emitted.javascript).unwrap();
        fs::write(dir.join("cases.json"), serde_json::to_vec(&cases).unwrap()).unwrap();
        fs::write(
            dir.join("run.mjs"),
            include_str!("fixtures/call_boundaries.mjs"),
        )
        .unwrap();
        let out = Command::new("node")
            .arg(dir.join("run.mjs"))
            .arg(mode)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{mode}: {}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        println!("{mode}: {}", String::from_utf8_lossy(&out.stdout));
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn large_interfaces_keep_compact_adapters_when_fixed_arity_emission_is_exhausted() {
    let mut source = "module Many;".to_owned();
    for i in 0..400 {
        source += &format!("fn routine{i}(x:f32,y:f32)->f32{{return x*y+x;}}");
    }
    let module = CheckedModule::from_source(parse(&source).unwrap()).unwrap();
    let emitted = ink_lowering_js::lower(&module).unwrap();
    assert!(
        emitted.javascript.contains("const needsInput=input.some"),
        "compact adapter fallback missing"
    );
    let dir = std::env::temp_dir().join(format!("ink-boundary-budget-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("program.mjs"), emitted.javascript).unwrap();
    fs::write(dir.join("run.mjs"),"import assert from 'node:assert/strict';import{functions}from './program.mjs';for(const n of [0,199,399])assert.equal(functions['routine'+n](.75,-.25),.5625);").unwrap();
    let out = Command::new("node")
        .arg(dir.join("run.mjs"))
        .output()
        .unwrap();
    fs::remove_dir_all(dir).unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn copier_namespaces_do_not_rename_source_fields_or_type_names() {
    let dir = std::env::temp_dir().join(format!("ink-copier-names-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let base="module names;record Row__ownedValueType{first__ownedValue:f32,second__ownedValue0:List<f32>,}fn copy(xs:List<Row__ownedValueType>)->List<Row__ownedValueType>{return xs;}";
    for mode in ["native", "captured", "bits", "wide"] {
        let mut source = base.to_owned();
        if mode == "wide" {
            source = source.replace("f32", "f64");
        }
        if matches!(mode, "captured" | "bits") {
            source += "state Counter:u32=0;";
        }
        if mode == "bits" {
            source += "fn owner(x:Buffer<f32>)->Buffer<f32>{return x;}";
        }
        let checked = CheckedModule::from_source(parse(&source).unwrap()).unwrap();
        fs::write(
            dir.join("program.mjs"),
            ink_lowering_js::lower(&checked).unwrap().javascript,
        )
        .unwrap();
        fs::write(dir.join("check.mjs"),r#"
import assert from 'node:assert/strict';import * as m from './program.mjs';
const wide=process.argv[2]==='wide',zero=wide?{F64Bits:'9223372036854775808'}:{F32Bits:0x80000000},nan=wide?{F64Bits:'9221120237041095220'}:{F32Bits:0x7f812345};
const input=[{first__ownedValue:zero,second__ownedValue0:[nan]}],expected=[{first__ownedValue:-0,second__ownedValue0:[nan]}],c=m.compiledValues.copy,h=c.params[0].capture(input);
assert.deepEqual(c.result.materialize(m.callCompiled('copy',[h])),input);
assert.deepEqual(m.callOwned('copy',[input]),input);
assert.deepEqual(m.functions.copy(input),expected);
const token=m.prepareCompiled('copy',[[0,h]]),out=['tail','tail'];
for(let i=0;i<3;i++){
 assert.equal(m.callPreparedCompiledInto(token,[],out),1);assert.deepEqual(out,[...expected,'tail']);
 out[0].first__ownedValue=99;out[0].second__ownedValue0.push(88);
 assert.deepEqual(c.result.materialize(m.callPreparedCompiled(token,[])),input);
}
const decoded=c.params[0].decode(c.params[0].encode(h));assert.deepEqual(c.params[0].materialize(decoded),input);
assert.deepEqual(input,[{first__ownedValue:zero,second__ownedValue0:[nan]}]);
"#).unwrap();
        let out = Command::new("node")
            .arg(dir.join("check.mjs"))
            .arg(mode)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{mode}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    fs::remove_dir_all(dir).unwrap();
}
