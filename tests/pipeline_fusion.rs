// Chains of map/filter/zip feeding sum, count, length or a new list compile to
// one loop with no intermediate lists. Each result must equal the reference
// evaluator's, through both trusted functions and checked calls.
use ink_core::{
    core::CheckedModule,
    eval,
    syntax::{parse, Type},
};
use serde_json::{json, Value};
use std::{fs, process::Command};

const SOURCE: &str = r#"module fusion;
record Item {id:u64,price:u64,qty:u64,open:Bool,weight:f32,}
fn sum32(xs:List<u32>,limit:u32)->u32{return sum(xs.filter(fn(x)=>x<limit).map(fn(x)=>x*17+3));}
fn sum64(xs:List<u64>,limit:u64)->u64{return sum(xs.filter(fn(x)=>x<limit).map(fn(x)=>x*17+3));}
fn sumi(xs:List<i32>)->i32{return sum(xs.map(fn(x)=>x*3).filter(fn(x)=>x>-100));}
fn sumf(xs:List<f32>)->f32{return sum(xs.filter(fn(x)=>x>0.5).map(fn(x)=>x*x));}
fn dot(xs:List<f32>,ys:List<f32>)->f32{return sum(xs.zip(ys,fn(a)=>fn(b)=>a*b));}
fn zip_kept(xs:List<u32>,ys:List<u32>)->List<u32>{return xs.zip(ys,fn(a)=>fn(b)=>a+b).filter(fn(s)=>s>10);}
fn zip_count(xs:List<u32>,ys:List<u32>)->u32{return length(xs.zip(ys,fn(a)=>fn(b)=>a*b));}
fn kept(xs:List<u32>)->List<u32>{return xs.filter(fn(x)=>x>3).map(fn(x)=>x+1);}
fn chain(xs:List<u64>)->List<u64>{return xs.map(fn(x)=>x*2).filter(fn(x)=>x>4).map(fn(x)=>x+7).filter(fn(x)=>x<1000);}
fn mapped_twice(xs:List<u32>)->List<u32>{return xs.map(fn(x)=>x+1).map(fn(x)=>x*x);}
fn how_many(xs:List<u32>)->u32{return length(xs.filter(fn(x)=>x>3).map(fn(x)=>x+1));}
fn counted(xs:List<u32>)->u64{return count(xs.map(fn(x)=>x+1).filter(fn(x)=>x>3));}
fn mapped_length(xs:List<u32>)->u32{return length(xs.map(fn(x)=>x+1));}
fn revenue(items:List<Item>)->u64{return sum(items.filter(fn(i)=>i.open).map(fn(i)=>i.price*i.qty));}
fn heavy(items:List<Item>)->List<u64>{return items.filter(fn(i)=>i.weight>0.5).map(fn(i)=>i.id);}
fn open_count(items:List<Item>)->u32{return length(items.filter(fn(i)=>i.open));}
fn nested(rows:List<List<u32>>)->List<u32>{return rows.map(fn(r)=>sum(r.filter(fn(x)=>x>2).map(fn(x)=>x*2))).filter(fn(s)=>s>0);}
"#;

fn wire(v: Value, t: &Type, p: &ink_core::syntax::Program) -> Value {
    match t {
        Type::U64 => json!(v.as_u64().unwrap().to_string()),
        Type::List(t) | Type::Vector(t, _) => Value::Array(v.as_array().unwrap().iter().map(|v| wire(v.clone(), t, p)).collect()),
        Type::Named(n) => p.records[n].iter().map(|(n, t)| (n.clone(), wire(v[n].clone(), t, p))).collect(),
        _ => v,
    }
}

#[test]
fn fused_pipelines_match_the_reference_evaluator() {
    let p = parse(SOURCE).unwrap();
    let module = CheckedModule::from_source(p.clone()).unwrap();
    let emitted = ink_lowering_js::lower(&module).unwrap();
    // Fused: no intermediate list is built by push or a sized array for
    // pipelines that end in a scalar sink.
    let body = |name: &str| {
        let code = &emitted.javascript;
        let index = p.functions.iter().position(|f| f.name == name).unwrap();
        let start = code.find(&format!("function __fn{index}(")).unwrap();
        let end = code[start..].find("\nfunction ").map_or(code.len(), |e| start + e);
        code[start..end].to_string()
    };
    for name in ["sum32", "sum64", "sumf", "dot", "revenue", "how_many", "counted", "open_count"] {
        let code = body(name);
        assert_eq!(code.matches("for(").count(), 1, "{name} is one loop:\n{code}");
        assert!(!code.contains(".push(") && !code.contains("new Array("), "{name} builds no list:\n{code}");
    }
    for name in ["kept", "chain", "mapped_twice", "zip_kept", "heavy"] {
        assert_eq!(body(name).matches("for(").count(), 1, "{name} is one loop");
    }
    assert_eq!(body("mapped_length").matches("for(").count(), 1, "a length after maps still runs each map once");

    let mut cases = vec![];
    let mut add = |name: &str, args: Value| {
        let f = p.functions.iter().find(|f| f.name == name).unwrap();
        let values = args.as_array().unwrap().iter().zip(&f.params).map(|(v, (_, t))| eval::Value::from_program_json(v, t, &p).unwrap()).collect();
        let expected = eval::call(&p, name, values, &mut 10_000_000).unwrap().json();
        let args = Value::Array(args.as_array().unwrap().iter().zip(&f.params).map(|(v, (_, t))| wire(v.clone(), t, &p)).collect());
        cases.push(json!({"name":name,"args":args,"expected":wire(expected,&f.result,&p)}));
    };
    let mut seed = 99u64;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for round in 0..60 {
        let n = (next() % 12) as usize;
        let words: Vec<u32> = (0..n).map(|_| [0, 1, 3, 4, 7, 2147483648, 4294967295u32][(next() % 7) as usize]).collect();
        let others: Vec<u32> = (0..(next() % 12) as usize).map(|_| (next() % 20) as u32).collect();
        let wide: Vec<u64> = words.iter().map(|w| [*w as u64, 18446744073709551615, 9007199254740993][(next() % 3) as usize]).collect();
        let signed: Vec<i32> = words.iter().map(|w| *w as i32).collect();
        let floats: Vec<f32> = (0..n).map(|_| [0.0f32, 0.75, -1.5, 3.4e38, 1e-45, 0.1][(next() % 6) as usize]).collect();
        let items: Vec<Value> = (0..n)
            .map(|i| {
                let price = [0u64, 7, 18446744073709551615][(next() % 3) as usize];
                let (qty, open) = (next() % 5, next() % 2 == 0);
                json!({"id":i,"price":price,"qty":qty,"open":open,"weight":floats.get(i).copied().unwrap_or(0.0)})
            })
            .collect();
        let limit = [0u32, 4, 2147483649, 4294967295][round % 4];
        add("sum32", json!([words, limit]));
        add("sum64", json!([wide, limit as u64]));
        add("sumi", json!([signed]));
        add("sumf", json!([floats]));
        add("dot", json!([floats, floats.iter().rev().collect::<Vec<_>>()]));
        add("zip_kept", json!([words, others]));
        add("zip_count", json!([words, others]));
        add("kept", json!([words]));
        add("chain", json!([others]));
        add("mapped_twice", json!([words]));
        add("how_many", json!([words]));
        add("counted", json!([words]));
        add("mapped_length", json!([words]));
        add("revenue", json!([items]));
        add("heavy", json!([items]));
        add("open_count", json!([items]));
        add("nested", json!([[words, others, []]]));
    }
    let dir = std::env::temp_dir().join(format!("ink-pipeline-fusion-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("program.mjs"), &emitted.javascript).unwrap();
    fs::write(dir.join("cases.json"), serde_json::to_vec(&cases).unwrap()).unwrap();
    fs::write(
        dir.join("test.mjs"),
        r#"
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {call,functions,metadata} from './program.mjs';
const cases=JSON.parse(readFileSync(new URL('./cases.json',import.meta.url)));
const named=metadata.named_types||{};
function native(v,t){switch(t.kind){case 'u64':return BigInt(v);case 'list':case 'vector':return v.map(x=>native(x,t.element));case 'ref':return native(v,named[t.name]);case 'record':return Object.fromEntries(t.fields.map(f=>[f.name,native(v[f.name],f.type)]));default:return v;}}
const json=v=>JSON.stringify(v,(_,v)=>typeof v==='bigint'?v.toString():v);
for(const {name,args,expected} of cases){
 const f=metadata.functions.find(f=>f.name===name),values=args.map((v,i)=>native(v,f.params[i].type));
 assert.deepEqual(JSON.parse(json(functions[name](...values))),JSON.parse(json(expected)),`${name}: ${json(args)}`);
 assert.deepEqual(JSON.parse(json(call(name,values))),JSON.parse(json(expected)),`call ${name}: ${json(args)}`);
}
console.log('cases',cases.length);
"#,
    )
    .unwrap();
    let out = Command::new("node").arg(dir.join("test.mjs")).output().unwrap();
    assert!(out.status.success(), "{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    fs::remove_dir_all(&dir).ok();
}
