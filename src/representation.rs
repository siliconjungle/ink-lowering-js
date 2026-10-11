//! Sealed representation execution with shared immutable constructor values.
use ink_core::{
    representation::{CheckedMigration, CheckedRepresentation},
    LangResult,
};
use serde_json::json;
pub fn lower(
    representations: &[CheckedRepresentation],
    migrations: &[CheckedMigration],
) -> LangResult<String> {
    if representations.is_empty() || representations.len() > 16 || migrations.len() > 16 {
        return Err("representation bundle limit".into());
    }
    let source = representations[0].module().identity()?;
    if representations
        .iter()
        .any(|r| r.module().identity().ok().as_ref() != Some(&source))
        || migrations.iter().any(|m| m.source_identity() != source)
    {
        return Err("representation source mismatch".into());
    }
    let mut emitted = String::new();
    let mut definitions = Vec::new();
    for d in representations
        .iter()
        .map(|r| r.definition())
        .chain(migrations.iter().map(|m| m.definition()))
    {
        let s = crate::definition::lower(d)?.replace("export ", "");
        let name = format!("definition_{}", definitions.len());
        emitted.push_str(&format!(
            "const {name}=(()=>{{\n{s}\nreturn {{identity,invoke}};}})();\n"
        ));
        definitions.push(name);
    }
    emitted.push_str(&format!("const definitions=[{}];\n", definitions.join(",")));
    let plans = representations
        .iter()
        .enumerate()
        .map(|(i, r)| {
            (
                r.request().representation.clone(),
                json!({"definition":i,"actions":r.plan().actions}),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    emitted.push_str(&format!("const plans={};\n", json!(plans)));
    emitted.push_str("export function execute(identity,functionId,args){const d=definitions.find(d=>d.identity===identity);if(!d)throw Error('sealed executable identity');return d.invoke(functionId,args);}\nexport function transition(representation,identity,action,args){const p=plans[representation],d=p&&definitions[p.definition],a=p&&p.actions[action];if(!a||d.identity!==identity)throw Error('sealed representation identity');const result=d.invoke(a.execute,args);return [d.invoke(a.observe,[result]),d.invoke(a.next,[result])];}\n");
    Ok(emitted)
}
