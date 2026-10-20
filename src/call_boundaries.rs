//! Fixed-arity public representation adapters. Conversion remains invocation
//! local; neither host object identity nor these adapters authorize retention.
use super::quote;
use serde_json::Value;

pub(super) enum Boundary {
    Public,
    Owned,
    Destination,
}

fn scalar_result(f: &Value) -> bool {
    matches!(
        f["result"]["kind"].as_str(),
        Some(
            "unit"
                | "bool"
                | "u32"
                | "i32"
                | "u64"
                | "f32"
                | "f64"
                | "int"
                | "string"
                | "id"
                | "enum"
        )
    )
}

fn generic(conversion: bool, boundary: &Boundary) -> String {
    let destination = matches!(boundary, Boundary::Destination);
    let public = matches!(boundary, Boundary::Public);
    if conversion && public {
        return r#"Object.freeze(Object.fromEntries(metadata.functions.filter(f=>!f.internal).map(f=> {
  const input=__inputTypes[f.index].types.map((t,i)=>__representation(t,false,__inputTypes[f.index].capture[i]));
  const output=__representation(f.result,true);
  const body=__implementations[f.index];
  const implementation=f.precondition==null?body:(...args)=>{if(__implementations[f.precondition](...args)!==true)throw Error('contract precondition failed');return body(...args);};
  const needsInput=input.some(c=>c!==__identity);
  const entry=!needsInput && output===__identity ? implementation :
    (...args)=>{const context=new WeakMap();return output(implementation(...(needsInput ? args.map((v,i)=>input[i](v,context)) : args)),context);};
  return [f.name,entry];
})))"#.into();
    }
    let filter = if destination {
        "!f.internal&&f.result.kind==='list'"
    } else {
        "!f.internal"
    };
    let body = if destination {
        "__intoImplementations[f.index]"
    } else {
        "__implementations[f.index]"
    };
    if !conversion {
        let check = if destination {
            "if(__implementations[f.precondition](...args.slice(0,f.params.length))!==true)throw Error('contract precondition failed');"
        } else {
            "if(__implementations[f.precondition](...args)!==true)throw Error('contract precondition failed');"
        };
        return format!("Object.freeze(Object.fromEntries(metadata.functions.filter(f=>{filter}).map(f=>{{const body={body};return [f.name,f.precondition==null?body:(...args)=>{{{check}return body(...args);}}];}})))");
    }
    let inputs = if conversion {
        "const input=__inputTypes[f.index].types.map((t,i)=>__representation(t,false,__inputTypes[f.index].capture[i]));const needsInput=input.some(c=>c!==__identity);"
    } else {
        "const needsInput=false;"
    };
    let output = if conversion && public {
        "const output=__representation(f.result,true);"
    } else {
        "const output=__identity;"
    };
    let check = if destination {
        "if(__implementations[f.precondition](...args.slice(0,f.params.length))!==true)throw Error('contract precondition failed');"
    } else {
        "if(__implementations[f.precondition](...args)!==true)throw Error('contract precondition failed');"
    };
    let convert = if destination {
        "input.map((c,i)=>c(args[i],context))"
    } else {
        "args.map((v,i)=>input[i](v,context))"
    };
    let call = if destination {
        "implementation(...values,args[f.params.length])"
    } else {
        "implementation(...values)"
    };
    let convert = if conversion { convert } else { "args" };
    format!("Object.freeze(Object.fromEntries(metadata.functions.filter(f=>{filter}).map(f=>{{{inputs}{output}const body={body};const implementation=f.precondition==null?body:(...args)=>{{{check}return body(...args);}};const entry=!needsInput&&output===__identity?implementation:(...args)=>{{const context=new WeakMap();const values=needsInput?{convert}:args;return output({call},context);}};return [f.name,entry];}})))")
}

pub(super) fn emit(functions: &[Value], conversion: bool, boundary: Boundary) -> String {
    // A direct caller can change V8's inlining of a collection-producing body.
    // Keep that established common adapter. Scalar results retain the bounded
    // fixed-arity lane; this is an emission heuristic, not a semantic fact.
    if conversion
        && matches!(boundary, Boundary::Public)
        && functions
            .iter()
            .any(|f| f["internal"] != true && !scalar_result(f))
    {
        let scalars = functions
            .iter()
            .filter(|f| scalar_result(f))
            .cloned()
            .collect::<Vec<_>>();
        let fixed = emit(&scalars, true, Boundary::Public);
        return format!(
            "Object.freeze({{...{},...{fixed}}})",
            generic(true, &Boundary::Public)
        );
    }
    let destination = matches!(boundary, Boundary::Destination);
    let output = matches!(boundary, Boundary::Public);
    let mut entries = Vec::new();
    let mut bytes = 0;
    for f in functions
        .iter()
        .filter(|f| f["internal"] != true && (!destination || f["result"]["kind"] == "list"))
    {
        let index = f["index"].as_u64().unwrap();
        let n = f["params"].as_array().unwrap().len();
        let vars = (0..n).map(|i| format!("a{i}")).collect::<Vec<_>>();
        let mut params = vars.clone();
        if destination {
            params.push("destination".into());
        }
        let body = format!("__{}{index}", if destination { "into" } else { "fn" });
        if !conversion && f["precondition"].is_null() {
            entries.push(format!("[{},{}]", quote(f["name"].as_str().unwrap()), body));
            continue;
        }
        let mut setup = String::new();
        let mut identity = Vec::new();
        let mut converted = Vec::new();
        for (i, var) in vars.iter().enumerate() {
            if conversion {
                setup += &format!("const c{i}=__representation(__inputTypes[{index}].types[{i}],false,__inputTypes[{index}].capture[{i}]);");
                identity.push(format!("c{i}===__identity"));
                converted.push(format!("c{i}({var},context)"));
            } else {
                converted.push(var.clone());
            }
        }
        if conversion && output {
            setup +=
                &format!("const output=__representation(metadata.functions[{index}].result,true);");
            identity.push("output===__identity".into());
        }
        if conversion {
            setup += &format!(
                "const input=[{}],needsInput=input.some(c=>c!==__identity);",
                (0..n)
                    .map(|i| format!("c{i}"))
                    .collect::<Vec<_>>()
                    .join(",")
            );
        }
        let composite = |t: &Value| {
            !matches!(
                t["kind"].as_str(),
                Some(
                    "unit"
                        | "bool"
                        | "u32"
                        | "i32"
                        | "u64"
                        | "f32"
                        | "f64"
                        | "int"
                        | "string"
                        | "id"
                        | "enum"
                )
            )
        };
        let inputs_composite = f["params"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .filter(|(_, p)| composite(&p["type"]))
            .map(|(i, _)| format!("c{i}!==__identity"))
            .collect::<Vec<_>>();
        let mut needs_context = inputs_composite;
        if conversion && output && composite(&f["result"]) {
            needs_context.push("output!==__identity".into());
        }
        let context = if !conversion || needs_context.is_empty() {
            "false".into()
        } else {
            format!("({})", needs_context.join("||"))
        };
        let actuals = (0..n).map(|i| format!("v{i}")).collect::<Vec<_>>();
        setup += &format!("const needsContext={context};");
        // Primitive arguments cannot alias a converted host object. Composite
        // traversals and multiple object arguments retain the original memo.
        let scalar_context = if conversion && vars.len() > 1 {
            format!(
                "||({})>1",
                (0..n)
                    .map(|i| format!(
                        "Number(c{i}!==__identity&&a{i}!==null&&typeof a{i}==='object')"
                    ))
                    .collect::<Vec<_>>()
                    .join("+")
            )
        } else {
            String::new()
        };
        let mut statements =
            format!("const context=(needsContext{scalar_context})?new WeakMap():null;");
        for (i, value) in converted.iter().enumerate() {
            statements += &format!("const v{i}={value};");
        }
        if !f["precondition"].is_null() {
            statements += &format!(
                "if(__fn{}({})!==true)throw Error('contract precondition failed');",
                f["precondition"],
                actuals.join(",")
            );
        }
        let mut values = actuals;
        if destination {
            values.push("destination".into());
        }
        let call = format!("{body}({})", values.join(","));
        statements += &format!(
            "return {};",
            if conversion && output {
                format!("output({call},context)")
            } else {
                call
            }
        );
        // Retain the existing variadic behavior for unusual trusted calls. The
        // ordinary exact-arity lane never builds an argument array or spreads.
        let fallback_input = if conversion {
            "needsInput?args.map((v,i)=>input[i](v,context)):args".into()
        } else {
            "args".to_owned()
        };
        let fallback_input = if destination {
            if conversion {
                "input.map((convert,i)=>convert(args[i],context))".into()
            } else {
                format!("args.slice(0,{n})")
            }
        } else {
            fallback_input
        };
        let fallback_check = if f["precondition"].is_null() {
            String::new()
        } else {
            format!(
                "if(__fn{}(...values)!==true)throw Error('contract precondition failed');",
                f["precondition"]
            )
        };
        let fallback_call = format!(
            "{body}(...values{})",
            if destination {
                format!(",args[{n}]")
            } else {
                String::new()
            }
        );
        let fallback = format!("const fallback=(...args)=>{{const context=new WeakMap();const values={fallback_input};{fallback_check}return {};}};",if conversion&&output{format!("output({fallback_call},context)")}else{fallback_call});
        let direct = if identity.is_empty() {
            "true".into()
        } else {
            identity.join("&&")
        };
        let direct = if f["precondition"].is_null() {
            format!("if({direct})return {body};")
        } else {
            String::new()
        };
        entries.push(format!("[{},(()=>{{{setup}{direct}{fallback}return function({}){{if(arguments.length!=={})return fallback(...arguments);{statements}}};}})()]",quote(f["name"].as_str().unwrap()),params.join(","),params.len()));
        bytes += entries.last().unwrap().len();
        // This lane is optional. Large interfaces retain the previous compact
        // adapters rather than acquiring a larger source admission requirement.
        if bytes > 262_144 {
            return generic(conversion, &boundary);
        }
    }
    format!(
        "Object.freeze(Object.fromEntries([{}]))",
        entries.join(",\n")
    )
}
