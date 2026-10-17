use ink_core::{
    action_model::{FunctionProjection, FunctionReplacement, FUNCTION_SEMANTICS},
    core::CheckedModule,
    logic::{Proof, Term},
    planning, registry, syntax,
};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn manifest_identifies_source_plans_with_frozen_float_rows() {
    for frozen in [false, true] {
        let make = |body: &str| {
            let mut program = syntax::parse(&format!(
                "module manifest;state Fixed:Table<u64,f32>=Table.empty();fn value(x:f32)->f32{{{body}}}"
            )).unwrap();
            if frozen {
                program.frozen.insert("Fixed".into(), vec![(json!(0), json!(0.1)), (json!(1), json!(-0.0))]);
            }
            CheckedModule::from_source(program).unwrap()
        };
        let original = make("let y=x;return y;");
        let selected = make("return x;");
        let names = BTreeSet::from(["value".to_string()]);
        let before = FunctionProjection::derive(&original, &names).unwrap();
        let after = FunctionProjection::derive(&selected, &names).unwrap();
        let mut objects = before.objects.clone();
        objects.extend(after.objects.clone());
        let entries = objects.keys().cloned().collect::<Vec<_>>();
        let knowledge = registry::Bundle {
            schema: 1,
            snapshot: registry::Snapshot {
                schema: 1, semantics: registry::SNAPSHOT_SEMANTICS.into(),
                root: registry::identity(&json!({"schema":1,"entries":entries})).unwrap(),
            },
            roots: entries.clone(),
            membership: entries.iter().map(|id| (id.clone(), registry::Membership {
                entries: entries.clone(), path: vec![],
            })).collect(),
            objects,
        };
        let arguments = before.functions["value"].params.iter()
            .map(|(name, _)| Term::Var(name.clone())).collect::<Vec<_>>();
        let call = |function: String| Term::Call { function, arguments: arguments.clone() };
        let package = planning::Package {
            schema: 1, semantics: planning::SEMANTICS.into(), input_core_sha256: original.identity().unwrap(),
            steps: vec![planning::Step::Functions(FunctionReplacement {
                schema: 1, semantics: FUNCTION_SEMANTICS.into(), input_core_sha256: original.identity().unwrap(),
                selected: serde_json::from_slice(&selected.bytes().unwrap()).unwrap(), knowledge,
                proofs: BTreeMap::from([("value".into(), Proof::Convert {
                    from: call(before.functions["value"].function.clone()),
                    to: call(after.functions["value"].function.clone()),
                })]),
            })],
        };
        let parsed = planning::from_bytes(&planning::canonical(&package).unwrap()).unwrap();
        let checked = planning::check(&original, &parsed).unwrap();
        let identity = planning::identity(&parsed).unwrap();
        if frozen {
            assert!(registry::identity(&parsed).unwrap_err().contains("floating JSON numbers"));
        } else {
            assert_eq!(registry::identity(&parsed).unwrap(), identity);
        }
        let emission = ink_lowering_js::lower_planned(&checked).unwrap();
        assert_eq!(emission.manifest["compiled_plan"], json!({
            "source": original.identity().unwrap(), "plan": identity,
        }));
        assert_eq!(checked.module().identity().unwrap(), selected.identity().unwrap());
    }
}
