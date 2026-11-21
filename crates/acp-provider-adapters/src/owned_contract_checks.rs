//! The owning check of every Router-owned contract the edge checker selects (D0 R52; correction 3,
//! item B): one registry that every part test and the live recording checker use, so a frame is
//! held to the same owning code wherever it is checked.
//!
//! Behind this crate's non-default `test-support` feature, enabled only from dev-dependencies:
//! production never builds it. It lives here because this crate already depends on every owner:
//! `session-event-model`'s profile types, and this crate's own Claude metadata and skill readers.
//! It names each contract as text (`RouterContract`'s camelCase form) and uses only plain function
//! types, so it needs no test-support dependency: test support reaches this crate through the CLI
//! and Host, and a dependency back would be a cycle. `OwnedCodecs::from_owner_registry` (test
//! support) turns these lists into the checker's registry.
//!
//! - Codecs that decode and re-encode: N9's `_session/*` bodies and the profile `_meta` of
//!   `initialize` and session results (`session_profile_codec.rs` types).
//! - Verdicts: `codexRouter.modelReasoning`, whose owning decoder has no encoder, is held to the
//!   agreement of its Zod schema (run through node) and the Rust decoder (addendum 2c).
//! - Decode-only checks: the skill annotations, read by `ClaudeAcpBehavior::observe`. The reader
//!   has no encoder, so a value it accepts conforms unchanged and one it calls malformed is a
//!   violation.

use acp_client_runtime::{ProviderBehavior, ProviderCommand, ProviderUpdate};
use provider_session_machines::{
    ModelReasoning, ModelReasoningEntry, ModelReasoningMetadata, ModelReasoningUnknown,
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use session_event_model::session_profile_codec::{
    InitializeProfileMetadata, QueueAddRequest, QueueAddResult, QueueCancelRequest,
    QueueCancelResult, QueueListRequest, QueueListResult, SessionResponseProfileMetadata,
    StateNotification, SteeringOutcome, SteeringRequest,
};
use session_event_model::{SkillSourceError, SkillView};

use acp_provider_adapters::{ClaudeAcpBehavior, ClaudeSkillSource};

/// A codec that decodes a raw value with the owning type and encodes it again.
pub type RoundTrip = fn(&Value) -> Result<Value, String>;

/// The owning Zod schema's verdict and the Rust decoder's outcome on one raw value, `(zod, rust)`.
pub type Verdicts = fn(&Value) -> Result<(Value, Value), String>;

/// Every round-trip and decode-only owning check, by contract name.
#[must_use]
pub fn round_trips() -> Vec<(&'static str, RoundTrip)> {
    vec![
        ("steeringParams", round_trip::<SteeringRequest>),
        ("steeringResult", round_trip::<SteeringOutcome>),
        ("queueAddParams", round_trip::<QueueAddRequest>),
        ("queueAddResult", round_trip::<QueueAddResult>),
        ("queueListParams", round_trip::<QueueListRequest>),
        ("queueListResult", round_trip::<QueueListResult>),
        ("queueCancelParams", round_trip::<QueueCancelRequest>),
        ("queueCancelResult", round_trip::<QueueCancelResult>),
        ("stateParams", round_trip::<StateNotification>),
        ("initializeMeta", round_trip::<InitializeProfileMetadata>),
        (
            "sessionResultMeta",
            round_trip::<SessionResponseProfileMetadata>,
        ),
        ("skillListMeta", skill_list_status),
        ("skillCommandMeta", skill_command_kind),
    ]
}

/// Every verdict owning check, by contract name.
#[must_use]
pub fn verdicts() -> Vec<(&'static str, Verdicts)> {
    vec![("modelReasoningMeta", model_reasoning_verdicts)]
}

/// Decodes `raw` with the owning type `T` and encodes it again.
fn round_trip<T: Serialize + DeserializeOwned>(raw: &Value) -> Result<Value, String> {
    let decoded: T = serde_json::from_value(raw.clone()).map_err(|error| error.to_string())?;
    serde_json::to_value(decoded).map_err(|error| error.to_string())
}

// Skill annotations: the reader `ClaudeAcpBehavior::observe` uses for an available_commands_update.

/// What the Claude reader makes of an `available_commands_update` with `commands` and the update
/// `_meta` `list_meta`.
fn skill_view(commands: Vec<ProviderCommand>, list_meta: Value) -> Result<SkillView, String> {
    ClaudeAcpBehavior::new(ClaudeSkillSource::Annotated)
        .observe(ProviderUpdate::Commands {
            commands,
            metadata: Some(list_meta),
        })
        .map_err(|error| format!("the skill reader failed: {error:?}"))?
        .skill_view
        .ok_or_else(|| "the skill reader returned no skill view".to_owned())
}

/// The update's `_meta.codexRouter` (the skill list status), held to the reader. Only a status the
/// reader reads (`available`, or `unavailable` as the provider reporting skills unavailable) is
/// accepted; a missing status reads as `Unknown` and malformed metadata as
/// `Unavailable(MalformedMetadata)`, and neither conforms.
fn skill_list_status(raw: &Value) -> Result<Value, String> {
    match skill_view(Vec::new(), json!({"codexRouter": raw}))? {
        SkillView::Available { .. }
        | SkillView::Unavailable(SkillSourceError::ProviderUnavailable) => Ok(raw.clone()),
        SkillView::Unknown => {
            Err("the skill reader finds no skill status in this list annotation".to_owned())
        }
        SkillView::Unavailable(reason) => Err(format!(
            "the skill reader rejects this list annotation: {reason:?}"
        )),
    }
}

/// One command's `_meta.codexRouter` (its skill kind), held to the reader under an available list.
fn skill_command_kind(raw: &Value) -> Result<Value, String> {
    let command = ProviderCommand {
        invocation_name: "command".to_owned(),
        description: String::new(),
        input_hint: None,
        metadata: Some(json!({"codexRouter": raw})),
    };
    let view = skill_view(
        vec![command],
        json!({"codexRouter": {"skillStatus": "available"}}),
    )?;
    // Only a command the reader takes as a skill, with no row error, is accepted: malformed or
    // unrecognized metadata, or an annotation naming no kind, is reported as that outcome.
    match &view {
        SkillView::Available { skills, errors } if errors.is_empty() && !skills.is_empty() => {
            Ok(raw.clone())
        }
        SkillView::Available { errors, .. } if !errors.is_empty() => Err(format!(
            "the skill reader does not accept this command annotation: {:?}",
            errors.iter().map(|error| &error.reason).collect::<Vec<_>>()
        )),
        other => Err(format!(
            "the skill reader does not take this command annotation as a skill: {other:?}"
        )),
    }
}

// `codexRouter.modelReasoning`: the Zod verdict against the Rust decoder's outcome.

fn repository_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Both owners' rejection, compared as one value: the two owners name their reasons differently,
/// so agreeing to reject is the comparable fact.
fn rejected() -> Value {
    json!({ "rejected": true })
}

/// The owning Zod schema's verdict on `raw`, run on the real schema through node.
fn zod_verdict(raw: &Value) -> Result<Value, String> {
    let root = repository_root();
    let output = std::process::Command::new("node")
        .arg(root.join("crates/acp-provider-adapters/tests/support/model-reasoning-zod-verdict.ts"))
        .arg(raw.to_string())
        .current_dir(&root)
        .output()
        .map_err(|error| format!("run node on the Zod verdict: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "the Zod verdict failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let verdict: Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("the Zod verdict is not JSON: {error}"))?;
    Ok(if verdict.get("rejected").is_some() {
        rejected()
    } else {
        verdict
    })
}

fn reason_value(reason: ModelReasoningUnknown) -> Result<Value, String> {
    serde_json::to_value(reason).map_err(|error| format!("encode {reason:?}: {error}"))
}

/// One decoded model row, in the annotation's terms, read through the public getters.
fn entry_value(entry: &ModelReasoningEntry) -> Result<Value, String> {
    Ok(match entry.reasoning() {
        ModelReasoning::Unknown(reason) => json!({
            "modelId": entry.model_id(), "kind": "unknown", "reason": reason_value(*reason)?
        }),
        ModelReasoning::Select(select) => json!({
            "modelId": entry.model_id(),
            "kind": "select",
            "configId": select.config_id(),
            "choices": select.choices().iter()
                .map(|choice| json!({ "value": choice.value, "label": choice.label }))
                .collect::<Vec<_>>(),
            "defaultValue": select.default_value(),
        }),
    })
}

/// The Rust decoder's outcome, read through its public getters. `Available` is only decoded from
/// version 1, so it is accepted as that version. A whole-value `Unknown` with a fallback reason
/// (`Malformed`, `Unrecognized`, `Ambiguous`) cannot say whether it decoded a valid
/// `kind: "unknown"` annotation or is the decoder's fallback for a value it refused
/// (`decode_claude_models` folds every failure into `Unknown`, model_reasoning_metadata.rs:97, a D1
/// finding): it is reported `ambiguous`, never guessed (correction 4, V2). Any other reason is an
/// accepted annotation.
fn rust_outcome(metadata: &ModelReasoningMetadata) -> Result<Value, String> {
    Ok(match metadata {
        ModelReasoningMetadata::Available {
            per_model_descriptors,
        } => json!({ "accepted": {
            "version": 1,
            "kind": "available",
            "models": per_model_descriptors.iter().map(entry_value).collect::<Result<Vec<_>, _>>()?,
        } }),
        ModelReasoningMetadata::Unknown(
            reason @ (ModelReasoningUnknown::Malformed
            | ModelReasoningUnknown::Unrecognized
            | ModelReasoningUnknown::Ambiguous),
        ) => json!({ "ambiguous": { "unknown": reason_value(*reason)? } }),
        ModelReasoningMetadata::Unknown(reason) => json!({ "accepted": {
            "version": 1, "kind": "unknown", "reason": reason_value(*reason)?
        } }),
    })
}

/// The owning check of a `modelReasoning` body: the Zod verdict and the Rust outcome.
fn model_reasoning_verdicts(raw: &Value) -> Result<(Value, Value), String> {
    let decoded = ClaudeAcpBehavior::new(ClaudeSkillSource::Annotated).decode_model_metadata(raw);
    Ok((zod_verdict(raw)?, rust_outcome(&decoded)?))
}
