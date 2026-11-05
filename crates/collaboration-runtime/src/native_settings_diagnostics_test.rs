use super::{Outcome, Presence, Projection, Reply, Token, project};
use crate::app_server_model_catalog::ProviderModelEntry;
use serde_json::{Value, json};

fn catalog(id: &str) -> Vec<ProviderModelEntry> {
    vec![
        ProviderModelEntry::try_new(id.into(), "Known".into(), String::new())
            .expect("fixture model"),
    ]
}

fn request(params: Value) -> Value {
    json!({"id":7,"method":"thread/settings/update","params":params})
}

#[test]
fn native_settings_diagnostics_distinguishes_absence_null_and_known_values() {
    let models = catalog("known");
    for (params, expected_model, expected_effort, expected_tier) in [
        (json!({}), Token::Absent, Token::Absent, Presence::Absent),
        (
            json!({"model":null,"effort":null,"serviceTier":null}),
            Token::Null,
            Token::Null,
            Presence::Null,
        ),
        (
            json!({"model":"known","effort":"provider","serviceTier":"private"}),
            Token::Known("known"),
            Token::Known("provider"),
            Presence::Value,
        ),
    ] {
        let request = request(params);
        let response = json!({"id":7,"result":{}});
        let Some(Projection::Settings {
            request_id,
            outcome,
            model,
            effort,
            service_tier,
            ..
        }) = project(&request, Reply::Response(&response), &models)
        else {
            panic!("settings projection");
        };
        assert_eq!(request_id, 7);
        assert_eq!(outcome, Outcome::Success);
        assert_eq!(model, expected_model);
        assert_eq!(effort, expected_effort);
        assert_eq!(service_tier, expected_tier);
    }
}

#[test]
fn native_settings_diagnostics_never_retains_secret_or_unadvertised_payloads() {
    let secret = "SECRET_SENTINEL_NEVER_LOG";
    let mut request = request(json!({
        "model":secret,"effort":secret,"serviceTier":secret,"cwd":secret,
        "threadId":secret,"actor":secret,"metadata":secret,"input":[{"text":secret}],
        "collaborationMode":{"mode":secret,"settings":{
            "model":secret,"reasoning_effort":secret,"developer_instructions":secret}}
    }));
    request["unexpected"] = json!(secret);
    let response = json!({"id":7,"error":{"code":-32000,"message":secret,"data":secret}});
    let projected =
        project(&request, Reply::Response(&response), &catalog("known")).expect("projection");
    let text = format!("{projected:?}");
    assert!(!text.contains(secret));
    assert!(text.contains("Redacted"));
    assert!(text.len() < 1500);
    let Projection::Settings {
        developer_instructions,
        service_tier,
        outcome,
        ..
    } = projected
    else {
        panic!("settings");
    };
    assert_eq!(developer_instructions, Presence::Value);
    assert_eq!(service_tier, Presence::Value);
    assert_eq!(outcome, Outcome::Error(-32000));
}

#[test]
fn native_settings_diagnostics_caps_known_models_and_redacts_malformed_tokens() {
    let long = "m".repeat(129);
    let models = catalog(&long);
    for value in [
        json!(long),
        json!({"private":"sentinel"}),
        json!(false),
        json!(["sentinel"]),
    ] {
        let request = request(json!({"model":value,"effort":value}));
        let response = json!({"id":7,"result":{}});
        let Some(Projection::Settings { model, effort, .. }) =
            project(&request, Reply::Response(&response), &models)
        else {
            panic!("settings");
        };
        assert_eq!(model, Token::Redacted);
        assert_eq!(effort, Token::Redacted);
    }
}

#[test]
fn native_settings_diagnostics_nested_values_and_early_failures_are_redacted() {
    let request = request(json!({"collaborationMode":{"mode":"default","settings":{
        "model":"known","reasoning_effort":"high","developer_instructions":null}}}));
    let response = json!({"id":7,"error":{"code":-32000,"message":"SECRET"}});
    let Some(Projection::Settings {
        default_mode,
        nested_model,
        nested_effort,
        developer_instructions,
        outcome,
        ..
    }) = project(&request, Reply::Response(&response), &catalog("known"))
    else {
        panic!("settings");
    };
    assert!(default_mode);
    assert_eq!(nested_model, Token::Known("known"));
    assert_eq!(nested_effort, Token::Known("high"));
    assert_eq!(developer_instructions, Presence::Null);
    assert_eq!(outcome.class(), "server_error");
    assert_eq!(outcome.code(), Some(-32000));
    for reply in [
        Reply::CatalogUnavailable(&response),
        Reply::DispatchUnavailable,
    ] {
        let projected = project(&request, reply, &[]).expect("early failure");
        let text = format!("{projected:?}");
        assert!(!text.contains("known"));
        assert!(!text.contains("SECRET"));
        assert!(matches!(
            projected,
            Projection::Settings {
                nested_model: Token::Redacted,
                ..
            }
        ));
    }
}

#[test]
fn native_settings_diagnostics_captures_only_selected_methods_and_matching_numeric_ids() {
    let models = catalog("known");
    let response = json!({"id":7,"result":{}});
    for method in ["turn/start", "thread/resume", "config/write", "initialize"] {
        let request = json!({"id":7,"method":method,"params":{"input":"SECRET"}});
        assert!(project(&request, Reply::Response(&response), &models).is_none());
    }
    let mut request = request(json!({"model":"known"}));
    assert!(
        project(
            &request,
            Reply::Response(&json!({"id":8,"result":{}})),
            &models
        )
        .is_none()
    );
    request["id"] = json!("SECRET");
    assert!(
        project(
            &request,
            Reply::Response(&json!({"id":"SECRET","result":{}})),
            &models
        )
        .is_none()
    );
}

#[test]
fn native_settings_diagnostics_default_save_records_outcome_only() {
    let request = json!({"id":7,"method":"config/batchWrite","params":{
        "edits":[{"value":"SECRET"}],"filePath":"SECRET","expectedVersion":"SECRET"}});
    let response = json!({"id":7,"result":{"filePath":"SECRET","version":"SECRET"}});
    let projected =
        project(&request, Reply::Response(&response), &catalog("known")).expect("batch outcome");
    assert_eq!(
        projected,
        Projection::DefaultSave {
            request_id_kind: "integer",
            outcome: Outcome::Success
        }
    );
    assert!(!format!("{projected:?}").contains("SECRET"));
}

#[test]
fn native_settings_diagnostics_matches_string_save_id_without_retaining_it() {
    // Arrange: pinned Codex config_update::write_config_batch uses this string shape.
    let id = "tui-config-write-1f4f1dd3-5818-41d5-a6bf-74365e89ee50";
    let request = json!({"id":id,"method":"config/batchWrite","params":{
        "edits":[{"value":"SECRET"}],"reloadUserConfig":true}});
    let response = json!({"id":id,"result":{"filePath":"SECRET"}});

    // Act: correlate the exact opaque string without retaining its contents.
    let projected = project(&request, Reply::Response(&response), &[])
        .expect("real Codex string-id default save must be observed");

    // Assert: neither request ID nor payload escapes; a different reply cannot match.
    let text = format!("{projected:?}");
    assert!(!text.contains(id));
    assert!(!text.contains("SECRET"));
    assert!(
        project(
            &request,
            Reply::Response(&json!({"id":"different","result":{}})),
            &[]
        )
        .is_none()
    );
}
