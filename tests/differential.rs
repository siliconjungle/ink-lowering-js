use ink_core::{
    core::CheckedModule,
    eval,
    syntax::{parse, Type},
};
use serde_json::{json, Value};
use std::{fs, process::Command};
fn wire(v: Value, t: &Type, p: &ink_core::syntax::Program) -> Value {
    match t {
        Type::U64 => json!(v.as_u64().unwrap().to_string()),
        Type::List(t) | Type::Vector(t, _) => Value::Array(
            v.as_array()
                .unwrap()
                .iter()
                .map(|v| wire(v.clone(), t, p))
                .collect(),
        ),
        Type::Named(n) => p.records[n]
            .iter()
            .map(|(n, t)| (n.clone(), wire(v[n].clone(), t, p)))
            .collect(),
        _ => v,
    }
}
#[test]
fn generated_javascript_matches_reference_and_rejects_invalid_inputs() {
    let source = format!(
        "{}\n{}",
        include_str!("../examples/particles.ink"),
        r#"
fn mul(x:u32,y:u32)->u32{return x*y;}
fn signed(x:i32,y:i32)->i32{return x*y+x-y;}
fn wide(x:u64,y:u64)->u64{return x*y+x-y;}
fn wide_scan(xs:List<u64>)->List<u64>{return xs.scan();}
fn wide_sort(xs:List<u64>)->List<u64>{return xs.sort();}
fn scalar_div(x:i32,y:i32)->i32{return quot_or(x,y,-99);}
fn remainder(x:i32,y:i32)->i32{return rem_or(x,y,-99);}
fn wide_div(x:u64,y:u64)->u64{return quot_or(x,y,99);}
fn wide_rem(x:u64,y:u64)->u64{return rem_or(x,y,99);}
fn narrow_div(x:u32,y:u32)->u32{return quot_or(x,y,99);}
fn count_flags(xs:List<Bool>)->u64{return count(xs);}
fn negate(x:f32)->f32{return -x;}
fn fadd(x:f32,y:f32)->f32{return x+y;}
fn fmul(x:f32,y:f32)->f32{return x*y;}
fn compare(x:f32,y:f32)->Bool{return x==y;}
fn less(x:u64,y:u64)->Bool{return x<y;}
fn lazy(x:u32)->u32{return choose(true,x,repeat(65536,x,fn(i)=>fn(a)=>repeat(65536,a,fn(j)=>fn(b)=>b+1)));}
fn lazy_at(xs:List<u32>)->u32{return xs.at_or(0,repeat(65536,0,fn(i)=>fn(a)=>repeat(65536,a,fn(j)=>fn(b)=>b+1)));}
fn lazy_div(x:u32)->u32{return quot_or(x,1,repeat(65536,0,fn(i)=>fn(a)=>repeat(65536,a,fn(j)=>fn(b)=>b+1)));}
fn bool_lazy()->Bool{return false && (repeat(65536,0,fn(i)=>fn(a)=>repeat(65536,a,fn(j)=>fn(b)=>b+1))==0);}
fn nested(xs:List<List<u32>>)->List<u32>{return xs.map(fn(xs)=>sum(xs));}
fn extrahelper(x:u32)->u32{return x+1;}
fn shadow(xs:List<u32>)->List<u32>{return xs.map(fn(x)=>choose(x>0,extrahelper(x),0));}
fn binding(x:u32)->u32{let x:u32=x+1; return x*3;}
fn fold(xs:List<u32>)->u32{return foldr(xs,0,fn(x)=>fn(rest)=>x-rest);}
fn dangerous()->u32{return repeat(65536,0,fn(i)=>fn(a)=>repeat(65536,a,fn(j)=>fn(b)=>b+1));}
fn default(x:u32)->u32{return x;}
fn __in(x:u32)->u32{return x+1;}
"#
    );
    let p = parse(&source).unwrap();
    let module = CheckedModule::from_source(p.clone()).unwrap();
    let emitted = ink_lowering_js::lower(&module).unwrap();
    let mut inputs = vec![];
    let mut add = |name: &str, args: Value| {
        let f = p.functions.iter().find(|f| f.name == name).unwrap();
        let values = args
            .as_array()
            .unwrap()
            .iter()
            .zip(&f.params)
            .map(|(v, (_, t))| eval::Value::from_program_json(v, t, &p).unwrap())
            .collect();
        let expected = eval::call(&p, name, values, &mut 10_000_000)
            .unwrap()
            .json();
        let args = Value::Array(
            args.as_array()
                .unwrap()
                .iter()
                .zip(&f.params)
                .map(|(v, (_, t))| wire(v.clone(), t, &p))
                .collect(),
        );
        inputs.push(json!({"name":name,"args":args,"expected":wire(expected,&f.result,&p)}));
    };
    for x in [0, 1, 65535, 65536, 2147483647, 2147483648, 4294967295u32] {
        for y in [0, 1, 65535, 65536, 2147483648, 4294967295u32] {
            add("mul", json!([x, y]));
            add("signed", json!([x as i32, y as i32]));
            add("scalar_div", json!([x as i32, y as i32]));
            add("remainder", json!([x as i32, y as i32]));
            add("narrow_div", json!([x, y]));
        }
    }
    for x in [
        0,
        1,
        9007199254740991,
        9007199254740993,
        18446744073709551615u64,
    ] {
        for y in [0, 1, 2, 18446744073709551615u64] {
            for name in ["wide", "wide_div", "wide_rem", "less"] {
                add(name, json!([x, y]));
            }
        }
    }
    let mut seed = 23u64;
    for _ in 0..200 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let x = seed;
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let y = seed;
        add("wide", json!([x, y]));
        add("mul", json!([x as u32, y as u32]));
        add("signed", json!([x as i32, y as i32]));
    }
    for xs in [
        json!([]),
        json!([0, 1, 2147483647, -2147483648i64, -1]),
        json!([-4, 7, 9]),
    ] {
        for n in ["indexed", "iterate", "prefix", "ordered"] {
            add(n, json!([xs]));
        }
        add("gather", json!([xs, [0, 1, 2, 999, 4294967295u32]]));
    }
    add(
        "wide_scan",
        json!([[18446744073709551615u64, 1, 9007199254740993u64]]),
    );
    add(
        "wide_sort",
        json!([[18446744073709551615u64, 1, 9007199254740993u64]]),
    );
    add("count_flags", json!([[true, false]]));
    add("kick", json!([[[1, 2, 3], [4, 5, 6]], 0.5, [0, -2, 0]]));
    add(
        "particles",
        json!([[{"position":[1,2,3],"velocity":[2,-2,1],"mass":5}],0.5]),
    );
    add("ordered_sum", json!([[16777216.0, 1.0, -16777216.0]]));
    add("ordered_sum", json!([[]]));
    for bits in [
        0,
        0x80000000,
        1,
        0x007fffff,
        0x00800000,
        0x7f800000,
        0xff800000,
        0x7fc12345u32,
    ] {
        add("negate", json!([{"F32Bits":bits}]));
        add("compare", json!([{"F32Bits":bits},{"F32Bits":bits}]));
    }
    for (a, b) in [
        (1.0000001192092896, 1.0000001192092896),
        (3.4028234663852886e38, 2.0),
        (1.401298464324817e-45, 0.5),
        (-0.0, 0.0),
    ] {
        add("fadd", json!([a, b]));
        add("fmul", json!([a, b]));
    }
    add("lazy", json!([7]));
    add("lazy_at", json!([[7]]));
    add("lazy_div", json!([7]));
    add("bool_lazy", json!([]));
    add("nested", json!([[[1, 2], [], [4294967295u32, 1]]]));
    add("shadow", json!([[0, 1, 4294967295u32]]));
    add("binding", json!([4294967295u32]));
    add("fold", json!([[1, 2, 3]]));
    add("default", json!([7]));
    add("__in", json!([7]));
    let dir = std::env::temp_dir().join(format!("ink-js-differential-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("program.mjs"), &emitted.javascript).unwrap();
    fs::write(dir.join("cases.json"), serde_json::to_vec(&inputs).unwrap()).unwrap();
    fs::write(dir.join("test.mjs"),r#"
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {call,functions,metadata} from './program.mjs';
const cases=JSON.parse(readFileSync(new URL('./cases.json',import.meta.url)));
const json=v=>JSON.stringify(v,(_,v)=>typeof v==='bigint'?v.toString():v);
for (const {name,args,expected} of cases) assert.deepEqual(JSON.parse(json(call(name,args))),JSON.parse(json(expected)),`${name}: ${json(args)}`);
for (const args of [[9007199254740992,0],[true,0],['0x10',0],[-1,0],['18446744073709551616',0]]) assert.throws(()=>call('wide',args));
assert.throws(()=>call('mul',[1.5,0])); assert.throws(()=>call('mul',[1]));assert.throws(()=>call('missing',[]));
assert.throws(()=>call('kick',[[[1,2]],1,[0,0,0]]));assert.throws(()=>call('negate',[Infinity]));
assert.throws(()=>call('particles',[[{position:[1,2,3],velocity:[1,2,3],mass:1,extra:0}],1]));
assert.throws(()=>call('dangerous',[]),/budget/);
assert.throws(()=>{metadata.functions[0].name='bad';});
assert.equal(functions.default(7),7);
assert.equal(functions.__in(7),8);
assert.ok(Object.is(call('negate',[0]),-0));
const xs=[3,1,2];call('ordered',[xs]);assert.deepEqual(xs,[3,1,2]);
console.log(`${cases.length} reference comparisons and boundary regressions passed`);
"#).unwrap();
    let output = Command::new("node")
        .arg(dir.join("test.mjs"))
        .output()
        .expect("Node.js required for JavaScript execution tests");
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    println!("{}", String::from_utf8_lossy(&output.stdout));
    fs::remove_dir_all(dir).unwrap();
}
#[test]
fn rejects_stateful_opaque_and_recursive_programs_without_changing_core_types() {
    for source in [
        "module X;state count:Int=0;query get()->Int{return count;}",
        "module X;fn f(x:Int)->Int{return x;}",
        "module X;record R{x:R,}fn f(x:R)->R{return x;}",
    ] {
        let p = parse(source).unwrap();
        if let Ok(module) = CheckedModule::from_source(p) {
            assert!(ink_lowering_js::lower(&module).is_err());
        }
    }
}
