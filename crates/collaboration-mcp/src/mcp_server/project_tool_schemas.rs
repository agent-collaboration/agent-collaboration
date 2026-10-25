//! Project tools retain their common envelope and only their reachable definitions.

use message_board::{ProjectTool, ProjectToolSelection};
use schemars::JsonSchema;
use serde_json::{Map, Value, json};
use std::{collections::BTreeSet, sync::Arc};

pub(super) fn board_input_schema<T: JsonSchema + 'static>(name: &str) -> Arc<Map<String, Value>> {
    let full = rmcp::handler::server::tool::schema_for_type::<T>();
    let Some(tool) = ProjectTool::from_name(name) else {
        return full;
    };
    let source = Value::Object((*full).clone());
    match narrow_project_schema(source, tool.selection()) {
        Some(schema) => Arc::new(schema),
        // An incompatible generated schema must not advertise other operations.
        // Every named tool's valid fixture challenges this invariant in tests.
        None => Arc::new(Map::from_iter([("not".to_owned(), json!({}))])),
    }
}

fn narrow_project_schema(
    mut root: Value,
    selection: ProjectToolSelection,
) -> Option<Map<String, Value>> {
    let mut operation = select_variant(
        root.pointer("/$defs/BoardOperation")?,
        selection.operation_kind(),
    )?;
    match selection {
        ProjectToolSelection::Operation { .. } => {}
        ProjectToolSelection::TaskAction { action } => {
            let selected_action =
                select_variant(root.pointer("/$defs/TaskAction")?, action.kind())?;
            operation
                .get_mut("properties")?
                .as_object_mut()?
                .insert("action".to_owned(), selected_action);
        }
        ProjectToolSelection::SubtaskCheck { checked } => {
            operation
                .pointer_mut("/properties/checked")?
                .as_object_mut()?
                .insert("const".to_owned(), json!(checked));
        }
    }
    root.get_mut("$defs")?
        .as_object_mut()?
        .insert("BoardOperation".to_owned(), operation);
    retain_reachable_definitions(root)
}

fn select_variant(schema: &Value, kind: &str) -> Option<Value> {
    schema
        .get("oneOf")?
        .as_array()?
        .iter()
        .find(|variant| {
            variant
                .pointer("/properties/kind/const")
                .and_then(Value::as_str)
                == Some(kind)
        })
        .cloned()
}

fn retain_reachable_definitions(mut root: Value) -> Option<Map<String, Value>> {
    let definitions = root.as_object_mut()?.remove("$defs")?;
    let definitions = definitions.as_object()?;
    let mut required = BTreeSet::new();
    collect_local_refs(&root, &mut required);
    let mut pending = required.iter().cloned().collect::<Vec<_>>();
    while let Some(name) = pending.pop() {
        let definition = definitions.get(&name)?;
        let mut referenced = BTreeSet::new();
        collect_local_refs(definition, &mut referenced);
        for referenced_name in referenced {
            if required.insert(referenced_name.clone()) {
                pending.push(referenced_name);
            }
        }
    }
    let mut retained = Map::new();
    for name in required {
        retained.insert(name.clone(), definitions.get(&name)?.clone());
    }
    let root = root.as_object_mut()?;
    root.insert("$defs".to_owned(), Value::Object(retained));
    Some(std::mem::take(root))
}

fn collect_local_refs(schema: &Value, names: &mut BTreeSet<String>) {
    if let Some(name) = schema
        .get("$ref")
        .and_then(Value::as_str)
        .and_then(|reference| reference.strip_prefix("#/$defs/"))
    {
        names.insert(name.replace("~1", "/").replace("~0", "~"));
    }
    match schema {
        Value::Object(fields) => {
            for value in fields.values() {
                collect_local_refs(value, names);
            }
        }
        Value::Array(values) => {
            for value in values {
                collect_local_refs(value, names);
            }
        }
        _ => {}
    }
}
