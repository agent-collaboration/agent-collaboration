use super::model_default_save_receipt;
use serde_json::{Value, json};

fn default_save_batch(effort: Value) -> Value {
    json!({"edits":[
        {"keyPath":"model","value":"haiku","mergeStrategy":"replace"},
        {"keyPath":"model_reasoning_effort","value":effort,"mergeStrategy":"replace"}
    ],"reloadUserConfig":true})
}

/// Oracle: native config_update.rs:87-104,155-172 at 360319d8 and M1's
/// explicit harmless receipt, including an effort clear and custom values.
#[test]
fn recognizes_both_edit_orders_with_nullable_and_provider_effort_values() {
    // Arrange: omission and explicit null both mean no file/version target.
    for effort in [
        Value::Null,
        json!("medium"),
        json!("provider"),
        json!("custom"),
    ] {
        for reverse in [false, true] {
            for explicit_nulls in [false, true] {
                let mut batch = default_save_batch(effort.clone());
                if reverse {
                    batch["edits"]
                        .as_array_mut()
                        .expect("fixture edits")
                        .reverse();
                }
                if explicit_nulls {
                    batch["filePath"] = Value::Null;
                    batch["expectedVersion"] = Value::Null;
                }

                // Act through the boundary recognizer, with no provider/filesystem dependency.
                let receipt = model_default_save_receipt(&batch).expect("recognized native batch");

                // Assert against the native DTO and checked consumer, not a helper-built oracle.
                assert_eq!(
                    serde_json::to_value(receipt).expect("receipt"),
                    json!({
                        "status":"ok","version":"acp-compat-noop-v1",
                        "filePath":"/dev/null","overriddenMetadata":null
                    })
                );
            }
        }
    }
}

#[test]
fn rejects_malformed_or_unrelated_batches_without_matching_a_subset() {
    // Arrange each independent countercase around the exact native batch.
    let original = default_save_batch(json!("medium"));
    let mut cases: Vec<(&str, Value)> = vec![
        ("non-object", Value::Null),
        ("empty batch", json!({"edits":[],"reloadUserConfig":true})),
    ];
    for (label, pointer, value) in [
        ("empty model", "/edits/0/value", json!("")),
        ("NUL model", "/edits/0/value", json!("ha\0iku")),
        ("null model", "/edits/0/value", Value::Null),
        ("numeric model", "/edits/0/value", json!(4)),
        ("empty effort", "/edits/1/value", json!("")),
        ("NUL effort", "/edits/1/value", json!("med\0ium")),
        ("boolean effort", "/edits/1/value", json!(true)),
        ("object effort", "/edits/1/value", json!({})),
        ("array effort", "/edits/1/value", json!([])),
        (
            "wrong model strategy",
            "/edits/0/mergeStrategy",
            json!("upsert"),
        ),
        (
            "wrong effort strategy",
            "/edits/1/mergeStrategy",
            json!("upsert"),
        ),
        ("duplicate model", "/edits/1/keyPath", json!("model")),
        (
            "duplicate effort",
            "/edits/0/keyPath",
            json!("model_reasoning_effort"),
        ),
        ("other setting", "/edits/1/keyPath", json!("features.hooks")),
        (
            "trust edit",
            "/edits/1/keyPath",
            json!("projects.fixture.trust_level"),
        ),
        ("reload false", "/reloadUserConfig", json!(false)),
        ("reload string", "/reloadUserConfig", json!("true")),
    ] {
        let mut batch = original.clone();
        *batch.pointer_mut(pointer).expect("fixture pointer") = value;
        cases.push((label, batch));
    }
    for (label, key, value) in [
        ("explicit file", "filePath", json!("/tmp/config.toml")),
        ("sink as explicit file", "filePath", json!("/dev/null")),
        ("empty file", "filePath", json!("")),
        ("wrong file type", "filePath", json!(false)),
        (
            "expected version",
            "expectedVersion",
            json!("saved-version"),
        ),
        (
            "receipt as version",
            "expectedVersion",
            json!("acp-compat-noop-v1"),
        ),
        ("empty version", "expectedVersion", json!("")),
        ("unrecognized intent field", "otherWriteIntent", json!(true)),
    ] {
        let mut batch = original.clone();
        batch[key] = value;
        cases.push((label, batch));
    }
    for (label, pointer, field) in [
        ("missing model value", "/edits/0", "value"),
        ("missing effort value", "/edits/1", "value"),
        ("missing strategy", "/edits/1", "mergeStrategy"),
        ("missing key", "/edits/0", "keyPath"),
        ("missing reload", "", "reloadUserConfig"),
    ] {
        let mut batch = original.clone();
        batch
            .pointer_mut(pointer)
            .expect("fixture object")
            .as_object_mut()
            .expect("fixture fields")
            .remove(field);
        cases.push((label, batch));
    }
    let mut extra = original.clone();
    extra["edits"]
        .as_array_mut()
        .expect("fixture edits")
        .push(json!({
            "keyPath":"features.hooks","value":true,"mergeStrategy":"replace"
        }));
    cases.push(("extra edit", extra));
    let mut missing = original;
    missing["edits"]
        .as_array_mut()
        .expect("fixture edits")
        .pop();
    cases.push(("missing edit", missing));

    for (label, batch) in cases {
        // Act / Assert: the full batch must match; unrelated work is never absorbed.
        assert!(model_default_save_receipt(&batch).is_none(), "{label}");
    }
}

#[test]
fn literal_backslash_zero_is_not_a_nul_character() {
    // Arrange / Act: these are ordinary nonempty native strings.
    let mut batch = default_save_batch(json!(r"custom\0effort"));
    batch["edits"][0]["value"] = json!(r"model\0name");

    // Assert: only an actual NUL is rejected; strings are not escape-scanned twice.
    assert!(model_default_save_receipt(&batch).is_some());
}
