use acp_client_runtime::{ProviderBehavior, ProviderCommand, ProviderUpdate};
use acp_provider_adapters::{ClaudeAcpBehavior, ClaudeSkillSource};
use session_event_model::{ProviderSkillScope, SkillView};

#[test]
fn actual_typescript_model_response_enters_real_rust_decoder() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = std::process::Command::new("node")
        .arg(root.join("providers/claude-acp/src/model-metadata-producer.ts"))
        .current_dir(&root)
        .output()
        .expect("actual TS producer");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("producer JSON");
    let metadata = response
        .pointer("/_meta/codexRouter/modelReasoning")
        .expect("initial response carrier");
    let facts =
        ClaudeAcpBehavior::new(ClaudeSkillSource::Annotated).decode_model_metadata(metadata);
    let acp_client_runtime::ModelReasoning::Select(select) = facts.for_model("sonnet") else {
        panic!("qualified descriptor")
    };
    assert_eq!(select.config_id(), "effort");
    assert_eq!(select.default_value(), "default");
    assert_eq!(
        select
            .choices()
            .iter()
            .map(|choice| choice.value.as_str())
            .collect::<Vec<_>>(),
        vec!["default", "low", "high"]
    );
    assert_eq!(
        facts.for_model("haiku"),
        acp_client_runtime::ModelReasoning::Unknown(
            acp_client_runtime::ModelReasoningUnknown::Unavailable
        )
    );
}

#[test]
fn actual_typescript_metadata_output_enters_real_rust_decoder() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = std::process::Command::new("node")
        .arg(root.join("providers/claude-acp/src/metadata-producer.ts"))
        .current_dir(&root)
        .output()
        .expect("run installed Node on actual TS metadata producer");
    assert!(
        output.status.success(),
        "TS producer failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut notification: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("real TS notification");
    notification
        .pointer_mut("/update/_meta/codexRouter")
        .and_then(serde_json::Value::as_object_mut)
        .expect("actual skill namespace")
        .insert(
            "modelReasoning".into(),
            serde_json::json!({"version":1,"kind":"available","models":"malformed"}),
        );
    let update = notification.get("update").expect("update");
    let rows = update
        .get("availableCommands")
        .and_then(serde_json::Value::as_array)
        .expect("rows");
    let commands = rows
        .iter()
        .map(|row| ProviderCommand {
            invocation_name: row
                .get("name")
                .and_then(serde_json::Value::as_str)
                .expect("name")
                .into(),
            description: row
                .get("description")
                .and_then(serde_json::Value::as_str)
                .expect("description")
                .into(),
            input_hint: None,
            metadata: row.get("_meta").cloned(),
        })
        .collect();
    let behavior = ClaudeAcpBehavior::new(ClaudeSkillSource::Annotated);
    assert_eq!(
        behavior.decode_model_metadata(
            update
                .pointer("/_meta/codexRouter/modelReasoning")
                .expect("malformed model carrier")
        ),
        acp_client_runtime::ModelReasoningMetadata::Unknown(
            acp_client_runtime::ModelReasoningUnknown::Malformed
        )
    );
    let facts = behavior
        .observe(ProviderUpdate::Commands {
            commands,
            metadata: update.get("_meta").cloned(),
        })
        .expect("decode produced metadata");
    let Some(SkillView::Available { skills, errors }) = facts.skill_view else {
        panic!("available TS-produced inventory");
    };
    assert!(errors.is_empty());
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].invocation_name.as_str(), "fixture-review");
    assert_eq!(skills[0].description, "Fixture review");
    assert_eq!(skills[0].scope, ProviderSkillScope::System);
}
