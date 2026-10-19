use ink_core::{core::CheckedModule, eval, syntax::parse};
use serde_json::{json, Value};
use std::{fs, process::Command};

#[test]
fn scalar_record_calls_share_exact_tangents_and_preserve_borrowed_getters() {
    let root = std::env::temp_dir().join(format!("ink-scalar-records-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let mut total = 0;
    for mode in ["borrowed", "captured", "bits", "wide"] {
        let mut source = include_str!("fixtures/scalar-records.ink").to_owned();
        if mode == "wide" {
            source = source.replace("f32", "f64");
        }
        if matches!(mode, "captured" | "bits") {
            source.push_str("\nstate Counter:u32=0;");
        }
        if mode == "bits" {
            source.push_str("\nfn owner(value:Buffer<f32>)->Buffer<f32>{return value;}");
        }
        let module = CheckedModule::from_source(parse(&source).unwrap()).unwrap();
        let emitted = ink_lowering_js::lower(&module).unwrap();
        assert!(
            emitted.javascript.contains("function __record0("),
            "{mode}: missing outlined record helper"
        );
        let mut words = vec![
            0u64, 1, 0x80000000, 0x80000001, 0x007fffff, 0x00800000, 0x3f800001, 0xbf800001,
            0x7f7fffff, 0xff7fffff, 0x7f800000, 0xff800000, 0x7f800001, 0x7fc12345, 0xffc12345,
        ];
        if mode == "wide" {
            words = vec![
                0,
                1,
                1 << 63,
                (1 << 63) + 1,
                0x0010000000000000,
                0x3ff0000000000001,
                0xbff0000000000001,
                0x7fefffffffffffff,
                0xffefffffffffffff,
                0x7ff0000000000000,
                0xfff0000000000000,
                0x7ff0000000001234,
                0xfff8000000005678,
            ];
        }
        let mut seed = 137u64;
        for _ in 0..24 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            words.push(if mode == "wide" {
                seed
            } else {
                seed as u32 as u64
            });
        }
        let scalar = |word: u64| {
            if mode == "wide" {
                json!({"F64Bits":word.to_string()})
            } else {
                json!({"F32Bits":word})
            }
        };
        let p = module.program();
        let mut cases = vec![];
        for i in 0..words.len() {
            let x = scalar(words[i]);
            let y = scalar(words[(i + 5) % words.len()]);
            let pair = json!({"a":x,"b":y});
            let bundle:Value=["p","q","r","s","t","u"].iter().enumerate().map(|(j,n)|(n.to_string(),json!({"a":scalar(words[(i+j)%words.len()]),"b":scalar(words[(i+j+2)%words.len()])}))).collect();
            for (name, args) in [
                ("derivatives", json!([bundle, x])),
                ("direction", json!([bundle, x])),
                ("twice", json!([pair])),
                ("branches", json!([x, i % 2 == 0])),
                ("looping", json!([x])),
                ("strict", json!([x, y])),
                ("history", json!([i])),
            ] {
                let f = p.functions.iter().find(|f| f.name == name).unwrap();
                let values = args
                    .as_array()
                    .unwrap()
                    .iter()
                    .zip(&f.params)
                    .map(|(v, (_, t))| eval::Value::from_program_json(v, t, p).unwrap())
                    .collect();
                let expected = eval::call_checked(&module, name, values, &mut 10_000_000)
                    .unwrap()
                    .json();
                cases.push(json!({"name":name,"args":args,"expected":expected}));
            }
        }
        fs::write(root.join("program.mjs"), &emitted.javascript).unwrap();
        fs::write(root.join("cases.json"), serde_json::to_vec(&cases).unwrap()).unwrap();
        fs::write(
            root.join("run.mjs"),
            include_str!("fixtures/scalar-records.mjs"),
        )
        .unwrap();
        let output = Command::new("node")
            .arg(root.join("run.mjs"))
            .arg(mode)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{mode}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        println!("{mode}: {}", String::from_utf8_lossy(&output.stdout));
        if let Some(out) = std::env::var_os("INK_SCALAR_RECORD_ARTIFACTS") {
            let out = std::path::PathBuf::from(out).join(mode);
            fs::create_dir_all(&out).unwrap();
            for name in ["program.mjs", "cases.json", "run.mjs"] {
                fs::copy(root.join(name), out.join(name)).unwrap();
            }
            fs::write(out.join("source.ink"), &source).unwrap();
            fs::write(out.join("source.core.json"), module.bytes().unwrap()).unwrap();
            fs::write(out.join("observations.json"), &output.stdout).unwrap();
        }
        total += cases.len();
    }
    println!("{total} exact reference cases across four carriers");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn oversized_optional_record_body_retains_an_ordinary_complete_call() {
    let text = "payload".repeat(10_000);
    let source=format!("module Oversized;record Pair{{a:f32,b:f32,}}fn bulky(r:Pair)->String{{return {text:?};}}fn run(x:f32)->String{{return bulky(Pair{{a:x,b:-x}});}}");
    let module = CheckedModule::from_source(parse(&source).unwrap()).unwrap();
    let emitted = ink_lowering_js::lower(&module).unwrap();
    let declaration = emitted
        .javascript
        .split("function __record0(")
        .nth(1)
        .unwrap();
    assert!(declaration
        .split('}')
        .next()
        .unwrap()
        .contains("return __fn0("));
    let root = std::env::temp_dir().join(format!("ink-record-fallback-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("program.mjs"), emitted.javascript).unwrap();
    fs::write(root.join("run.mjs"),"import assert from 'node:assert/strict';import {functions} from './program.mjs';for(const x of [0,-0,Infinity,{F32Bits:0x7f812345}])assert.equal(functions.run(x),'payload'.repeat(10000));console.log('oversized record helper fallback passed');").unwrap();
    let output = Command::new("node")
        .arg(root.join("run.mjs"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    fs::remove_dir_all(root).unwrap();
}
