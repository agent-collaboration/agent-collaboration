use schemars::{JsonSchema, generate::SchemaSettings};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::path::Path;
#[cfg(test)]
#[path = "negative_literals.rs"]
mod negative_literals;
#[cfg(test)]
#[path = "read_literals.rs"]
mod read_literals;
#[cfg(test)]
#[path = "schema_validation.rs"]
mod schema_validation;

pub fn assert_contract<T: JsonSchema + Serialize + DeserializeOwned>(
    name: &str,
    literals: Vec<Value>,
    open_root: bool,
) {
    let schema = serde_json::to_value(
        SchemaSettings::draft2020_12()
            .for_serialize()
            .into_generator()
            .into_root_schema_for::<T>(),
    )
    .unwrap();
    let validator = schema_validation::contract_validator(&schema).unwrap();
    assert_eq!(
        T::schema_name(),
        name,
        "artifact name must identify its Rust owner"
    );
    let mut negatives = negative_literals::explicit_negatives(name, &literals);
    let reads = read_literals::accepted_reads(name, &literals);
    for read in &reads {
        let input = &read["input"];
        let parsed: T = serde_json::from_value(input.clone()).unwrap();
        assert_eq!(serde_json::to_value(parsed).unwrap(), read["canonical"]);
        assert!(
            validator.is_valid(input),
            "{name} schema refused an accepted read"
        );
        if name == "MachineTokenClaims" {
            let mut missing = input.clone();
            missing.as_object_mut().unwrap().remove("principal");
            negatives.push(missing);
            for kind in ["session", "person", "service", "fourthKind"] {
                let mut wrong_kind = input.clone();
                wrong_kind["principal"]["kind"] = Value::String(kind.to_owned());
                negatives.push(wrong_kind);
            }
        }
    }
    let mut generated = Vec::new();
    for literal in literals {
        let parsed: T = serde_json::from_value(literal.clone())
            .unwrap_or_else(|error| panic!("{name}: {literal}: {error}"));
        let encoded = serde_json::to_value(parsed).unwrap();
        assert_eq!(
            encoded, literal,
            "{name} differs from the transcribed wire oracle"
        );
        assert!(
            validator.is_valid(&encoded),
            "{name} fixture fails its schema: {encoded}"
        );
        for invalid in malformed_bodies(&literal, open_root) {
            assert!(
                serde_json::from_value::<T>(invalid.clone()).is_err(),
                "{name} serde accepted {invalid}"
            );
            assert!(
                !validator.is_valid(&invalid),
                "{name} schema accepted {invalid}"
            );
            negatives.push(invalid);
        }
        generated.push(encoded);
    }
    for invalid in &negatives {
        assert!(
            serde_json::from_value::<T>(invalid.clone()).is_err(),
            "{name} serde accepted {invalid}"
        );
        assert!(
            !validator.is_valid(invalid),
            "{name} schema accepted {invalid}"
        );
    }
    let mut negatives = negatives.iter().map(canonical_value).collect::<Vec<_>>();
    negatives.sort_by_cached_key(|value| serde_json::to_string(value).unwrap());
    negatives.dedup();
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("contracts");
    check_artifact(&directory.join(format!("{name}.schema.json")), &schema);
    if !reads.is_empty() {
        check_artifact(
            &directory.join(format!("{name}.reads.json")),
            &serde_json::to_value(reads).unwrap(),
        );
    }
    check_artifact(
        &directory.join(format!("{name}.negatives.json")),
        &serde_json::to_value(negatives).unwrap(),
    );
    check_artifact(
        &directory.join(format!("{name}.fixtures.json")),
        &serde_json::to_value(generated).unwrap(),
    );
}

fn canonical_value(value: &Value) -> Value {
    match value {
        Value::Object(fields) => {
            let mut entries = fields.iter().collect::<Vec<_>>();
            entries.sort_unstable_by(|left, right| left.0.cmp(right.0));
            Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key.clone(), canonical_value(value)))
                    .collect(),
            )
        }
        Value::Array(items) => Value::Array(items.iter().map(canonical_value).collect()),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => value.clone(),
    }
}

fn check_artifact(path: &Path, actual: &Value) {
    if std::env::var_os("UPDATE_FLEET_IDENTITY_CONTRACTS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let output = if let Some(entries) = actual.as_array() {
            let lines = entries
                .iter()
                .map(|entry| serde_json::to_string(entry).unwrap())
                .collect::<Vec<_>>()
                .join(",\n");
            format!("[\n{lines}\n]\n")
        } else {
            format!("{}\n", serde_json::to_string_pretty(actual).unwrap())
        };
        std::fs::write(path, output).unwrap();
    } else {
        let expected: Value =
            serde_json::from_slice(&std::fs::read(path).unwrap_or_else(|error| {
                panic!(
                    "{}: {error}; regenerate with UPDATE_FLEET_IDENTITY_CONTRACTS=1",
                    path.display()
                )
            }))
            .unwrap();
        assert_eq!(&expected, actual, "{} is stale", path.display());
    }
}

fn malformed_bodies(literal: &Value, open_root: bool) -> Vec<Value> {
    fn visit(value: &Value, path: &str, paths: &mut Vec<String>) {
        match value {
            Value::Object(fields) => {
                paths.push(path.to_owned());
                for (key, child) in fields {
                    visit(
                        child,
                        &format!("{path}/{}", key.replace('~', "~0").replace('/', "~1")),
                        paths,
                    );
                }
            }
            Value::Array(items) => {
                for (index, child) in items.iter().enumerate() {
                    visit(child, &format!("{path}/{index}"), paths);
                }
            }
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
        }
    }
    let mut paths = Vec::new();
    visit(literal, "", &mut paths);
    let mut negatives = Vec::new();
    for path in paths {
        if open_root && path.is_empty() {
            continue;
        }
        let mut unknown = literal.clone();
        unknown
            .pointer_mut(&path)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("unexpectedField".to_owned(), Value::Bool(true));
        negatives.push(unknown);
        let fields = literal.pointer(&path).unwrap().as_object().unwrap();
        for key in fields.keys() {
            if key.chars().any(char::is_uppercase) {
                let snake = key
                    .chars()
                    .flat_map(|character| {
                        if character.is_uppercase() {
                            vec!['_', character.to_ascii_lowercase()]
                        } else {
                            vec![character]
                        }
                    })
                    .collect::<String>();
                let mut wrong_case = literal.clone();
                let fields = wrong_case
                    .pointer_mut(&path)
                    .unwrap()
                    .as_object_mut()
                    .unwrap();
                let value = fields.remove(key).unwrap();
                fields.insert(snake, value);
                negatives.push(wrong_case);
            }
            if matches!(
                key.as_str(),
                "kind" | "result" | "state" | "operation" | "subject"
            ) && fields[key].is_string()
            {
                let mut unknown_tag = literal.clone();
                unknown_tag.pointer_mut(&path).unwrap()[key] =
                    Value::String("unrecognizedVariant".to_owned());
                negatives.push(unknown_tag);
            }
        }
    }
    negatives
}
