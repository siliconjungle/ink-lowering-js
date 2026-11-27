use ink_core::{core::CheckedModule, eval, syntax::parse};
use serde_json::json;
use std::{fs, process::Command};

#[test]
fn copying_dense_slot_ranges_matches_the_interpreter() {
    let program = CheckedModule::from_source(parse(r#"module BufferCopies;
fn words(xs:List<u32>,from:u32,to:u32,n:u32)->List<u32>{
    return buffer(xs).copy_within(from,to,n).freeze();
}
fn ranges(xs:List<u32>,from:u32,to:u32,n:u32)->List<u32>{
    return buffer(xs).split_at(2,fn(left)=>fn(right)=>left.append(right.copy_within(from,to,n))).freeze();
}
fn records(xs:List<Option<u32>>,from:u32,to:u32,n:u32)->List<Option<u32>>{
    return buffer(xs).copy_within(from,to,n).freeze();
}
"#).unwrap()).unwrap();
    let mut cases = Vec::new();
    for name in ["words", "ranges", "records"] {
        let function = program.program().functions.iter().find(|f| f.name == name).unwrap();
        let input = if name == "records" {
            json!([{"Some":1},{"None":null},{"Some":3},{"Some":4},{"None":null},{"Some":6},{"Some":7},{"Some":8}])
        } else {
            json!([1,2,3,4,5,6,7,8])
        };
        for (source, target, count) in [(0, 1, 5), (1, 0, 5), (0, 5, 3), (5, 0, 3),
            (3, 9, 5), (8, 0, 99), (0, 3, 99), (2, 2, 4), (0, 1, 0)] {
            let args = json!([input, source, target, count]);
            let values = args.as_array().unwrap().iter().zip(&function.params)
                .map(|(v, (_, ty))| eval::Value::from_program_json(v, ty, program.program()).unwrap()).collect();
            let expected = eval::call_checked(&program, name, values, &mut 1_000_000).unwrap().json();
            cases.push(json!({"name":name,"args":args,"expected":expected}));
        }
    }
    let output = ink_lowering_js::lower(&program).unwrap().javascript;
    let root = std::env::temp_dir().join(format!("ink-buffer-copy-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("program.mjs"), output).unwrap();
    fs::write(root.join("cases.json"), serde_json::to_vec(&cases).unwrap()).unwrap();
    fs::write(root.join("test.mjs"), r#"
import assert from 'node:assert/strict';
import fs from 'node:fs';
import { functions } from './program.mjs';
for(const { name, args, expected } of JSON.parse(fs.readFileSync(new URL('cases.json', import.meta.url)))){
    assert.deepEqual(functions[name](...args),expected,JSON.stringify([name,args]));
}
"#).unwrap();
    let run = Command::new("node").arg(root.join("test.mjs")).output().unwrap();
    fs::remove_dir_all(root).unwrap();
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
}
