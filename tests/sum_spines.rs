use ink_core::{
    core::CheckedModule,
    logic::{self, Sort, Term},
    physical, planning, registry,
    sum_codec::{self, Schema},
    syntax::{parse, Type},
};
use std::{fs, process::Command};
#[path="support/record_codec.rs"] mod record_codec;

// Test authoring is separate from emission. Admission replays both ordinary
// equality proofs over the compiler's exact exported codec declarations.
fn var(name: &str) -> Term {
    Term::Var(name.into())
}
fn call(name: &str, argument: Term) -> Term {
    Term::Call {
        function: name.into(),
        arguments: vec![argument],
    }
}
fn node(datatype: &str, constructor: usize, argument: Term) -> Term {
    Term::Construct {
        datatype: datatype.into(),
        constructor,
        arguments: vec![argument],
    }
}
fn round(schema: &Schema, value: Term) -> Term {
    call(&schema.decode, call(&schema.encode, value))
}
fn embed(schema: &Schema, at: usize, mut value: Term) -> Term {
    for index in (0..at).rev() {
        value = node(&schema.source_chain[index], 1, value);
    }
    value
}
fn source_cases(schema: &Schema, at: usize, variable: &str) -> logic::Proof {
    let value = embed(schema, at, var(variable));
    let leaf = format!("leaf{at}");
    let tail = format!("tail{at}");
    let left = embed(schema, at, node(&schema.source_chain[at], 0, var(&leaf)));
    let right = embed(schema, at, node(&schema.source_chain[at], 1, var(&tail)));
    let right_proof = if at + 1 == schema.source_chain.len() {
        logic::Proof::Convert {
            from: round(schema, right.clone()),
            to: right,
        }
    } else {
        source_cases(schema, at + 1, &tail)
    };
    logic::Proof::Cases {
        variable: Some(variable.into()),
        scrutinee: None,
        from: round(schema, value.clone()),
        to: value,
        cases: vec![
            logic::Case {
                bindings: vec![leaf],
                proof: logic::Proof::Convert {
                    from: round(schema, left.clone()),
                    to: left,
                },
            },
            logic::Case {
                bindings: vec![tail],
                proof: right_proof,
            },
        ],
    }
}
fn layout(module: &CheckedModule, ty: &Type) -> sum_codec::Layout {
    let schema = sum_codec::schema(module, ty).unwrap();
    let source = var("source");
    let flat = var("flat");
    let inverse = |value: Term| call(&schema.encode, call(&schema.decode, value));
    let cases = (0..schema.alternatives.len())
        .map(|index| {
            let name = format!("leaf{index}");
            let value = node(&schema.flat, index, var(&name));
            logic::Case {
                bindings: vec![name],
                proof: logic::Proof::Convert {
                    from: inverse(value.clone()),
                    to: value,
                },
            }
        })
        .collect();
    let declarations = [
        logic::Declaration::Theorem {
            params: vec![("source".into(), schema.source.clone())],
            conditions: vec![],
            from: round(&schema, source.clone()),
            to: source,
            proof: source_cases(&schema, 0, "source"),
        },
        logic::Declaration::Theorem {
            params: vec![("flat".into(), Sort::Data(schema.flat.clone()))],
            conditions: vec![],
            from: inverse(flat.clone()),
            to: flat.clone(),
            proof: logic::Proof::Cases {
                variable: Some("flat".into()),
                scrutinee: None,
                from: inverse(flat.clone()),
                to: flat,
                cases,
            },
        },
    ];
    let mut objects = schema.entries.clone();
    let mut roots = vec![];
    for declaration in declarations {
        let entry = registry::Entry::from_declaration(
            &declaration,
            schema.entries.keys().cloned().collect(),
        )
        .unwrap();
        let ident = registry::identity(&entry).unwrap();
        objects.insert(ident.clone(), entry);
        roots.push(ident);
    }
    let roundtrip = roots[0].clone();
    let inverse = roots[1].clone();
    roots.sort();
    let ids = objects.keys().cloned().collect::<Vec<_>>();
    assert!(ids.len() <= 64);
    let knowledge=serde_json::from_value(serde_json::json!({
        "schema":1,"snapshot":{"schema":1,"semantics":registry::SNAPSHOT_SEMANTICS,
            "root":registry::identity(&serde_json::json!({"schema":1,"entries":ids})).unwrap()},
        "roots":roots,"objects":objects,
        "membership":ids.iter().map(|id|(id.clone(),serde_json::json!({"entries":ids,"path":[]}))).collect::<std::collections::BTreeMap<_,_>>()
    })).unwrap();
    sum_codec::Layout {
        sum_type: ty.clone(),
        knowledge,
        roundtrip,
        inverse,
    }
}

#[test]
fn sum_layouts_preserve_boundaries_transactions_and_exact_wire_resources() {
    let module =
        CheckedModule::from_source(parse(include_str!("fixtures/sum_runtime.ink")).unwrap())
            .unwrap();
    let layouts = sum_codec::source_types(&module)
        .unwrap()
        .iter()
        .filter(|ty| sum_codec::schema(&module, ty).is_ok())
        .map(|ty| layout(&module, ty))
        .collect::<Vec<_>>();
    assert!(layouts.len() >= 3);
    let directory = std::env::temp_dir().join(format!("ink-sum-spines-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let ordinary = ink_lowering_js::lower(&module).unwrap();
    fs::write(directory.join("runtime-ordinary.mjs"), &ordinary.javascript).unwrap();
    let choices = layouts
        .iter()
        .cloned()
        .map(|layout| vec![layout])
        .chain(std::iter::once(layouts.clone()));
    for (index, choice) in choices.enumerate() {
        let physical:physical::Plan=serde_json::from_value(serde_json::json!({
            "schema":1,"semantics":physical::SEMANTICS,"input_core_sha256":module.identity().unwrap(),"sum_spines":choice
        })).unwrap();
        let plan = planning::check(
            &module,
            &planning::Package {
                schema: 1,
                semantics: planning::SEMANTICS.into(),
                input_core_sha256: module.identity().unwrap(),
                steps: vec![planning::Step::Physical(physical)],
            },
        )
        .unwrap();
        assert!(plan
            .require_source_layout_capabilities(true, false)
            .is_err());
        let selected = ink_lowering_js::lower_planned(&plan).unwrap();
        assert_eq!(selected.manifest["wire"], ordinary.manifest["wire"]);
        let filename = if index == layouts.len() {
            "runtime-flat.mjs".into()
        } else {
            format!("runtime-one-{index}.mjs")
        };
        fs::write(directory.join(filename), selected.javascript).unwrap();
    }
    let elisions=module.program().records.keys()
        .filter(|name| ink_core::record_codec::schema(&module,name).is_ok())
        .map(|name|record_codec::layout(&module,name)).collect::<Vec<_>>();
    let physical:physical::Plan=serde_json::from_value(serde_json::json!({
        "schema":1,"semantics":physical::SEMANTICS,"input_core_sha256":module.identity().unwrap(),
        "sum_spines":layouts,"record_elisions":elisions
    })).unwrap();
    let composed=planning::check(&module,&planning::Package{
        schema:1,semantics:planning::SEMANTICS.into(),input_core_sha256:module.identity().unwrap(),
        steps:vec![planning::Step::Physical(physical)]
    }).unwrap();
    let code=ink_lowering_js::lower_planned(&composed).unwrap().javascript;
    fs::write(directory.join("runtime-composed.mjs"),code).unwrap();
    fs::write(
        directory.join("run.mjs"),
        include_str!("fixtures/sum_runtime.mjs"),
    )
    .unwrap();
    let result = Command::new("node")
        .arg(directory.join("run.mjs"))
        .output()
        .unwrap();
    fs::remove_dir_all(directory).unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}
