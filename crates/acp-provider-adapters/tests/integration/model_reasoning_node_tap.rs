//! N12 node taps over the real TypeScript producer: `providers/claude-acp/src/model-metadata-producer.ts`
//! runs the wrapper's own `ModelMetadataDecorator` (no model traffic) and prints the ACP response
//! that carries `_meta.codexRouter.modelReasoning`. Each case records two things N12 hands to N6:
//! first the producer's raw `modelReasoning` value, exactly as it printed it, as a Router-owned
//! `_meta` body (before Rust decodes it); then what the Rust reader N6 uses
//! (`ClaudeAcpBehavior::decode_model_metadata`) accepted, as its full typed `Debug` form. Both are
//! compared with whole expected values constructed from literals. The two retained-difference rows
//! mutate that real output.
//!
//! The body has a decoder and no encoder, so it is held to its two owners' verdicts: the owning Zod
//! schema (`providers/claude-acp/src/model-metadata.ts`, run by
//! `tests/support/model-reasoning-zod-verdict.ts`) and the Rust decoder, whose outcome is read
//! through its public getters. Those verdicts come from the shared owning-check registry
//! (`src/owned_contract_checks.rs`), the same one the live recording checker uses.
use acp_client_runtime::{
    ModelReasoning, ModelReasoningEntry, ModelReasoningMetadata, ModelReasoningSelect,
    ModelReasoningUnknown, ProviderBehavior, ProviderSettingChoice,
};
use acp_provider_adapters::{ClaudeAcpBehavior, ClaudeSkillSource};
use codex_router_test_support::node_taps::{
    BodyRoot, CatalogRowExpectation, EdgeParty, MessageFamily, NodeStateSnapshot, ProtocolNode,
    RecordIntent, RecordLayer, RouterContract, TapCase,
};
use serde_json::{Value, json};

const N6: EdgeParty = EdgeParty::Node(ProtocolNode::N6AcpClient);
const KIND: &str = "_meta.codexRouter.modelReasoning";
/// The producer's value as it crossed, before Rust decodes it.
const RAW_KIND: &str = "_meta.codexRouter.modelReasoning body";

/// The Router-owned `_meta` body layer of `codexRouter.modelReasoning`.
fn model_reasoning_body() -> RecordLayer {
    RecordLayer::Body {
        body: BodyRoot::RouterMeta {
            contract: RouterContract::ModelReasoningMeta,
        },
    }
}

fn repository_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The real producer's `modelReasoning` value.
#[expect(
    clippy::expect_used,
    reason = "test fixture: a producer that cannot run must fail the calling tap"
)]
fn produced_model_reasoning() -> Value {
    let root = repository_root();
    let output = std::process::Command::new("node")
        .arg(root.join("providers/claude-acp/src/model-metadata-producer.ts"))
        .current_dir(&root)
        .output()
        .expect("run node on the actual TS model metadata producer");
    assert!(
        output.status.success(),
        "TS producer failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: Value = serde_json::from_slice(&output.stdout).expect("producer JSON");
    response
        .pointer("/_meta/codexRouter/modelReasoning")
        .cloned()
        .expect("initial response carries modelReasoning")
}

fn decode(meta: &Value) -> ModelReasoningMetadata {
    ClaudeAcpBehavior::new(ClaudeSkillSource::Annotated).decode_model_metadata(meta)
}

/// The whole typed value, with no field dropped.
fn whole<T: std::fmt::Debug>(value: &T) -> Value {
    json!({ "debug": format!("{value:?}") })
}

/// How a case's raw body came to be.
enum BodyOrigin {
    /// The producer's own output.
    Producer,
    /// A test mutation both owners reject: a deliberately invalid value whose owned check must show
    /// `bothReject` (correction 4, V2: agreement on rejection certifies nothing).
    InjectedBothReject,
    /// A test mutation Zod accepts and the Rust decoder folds into a fallback `Unknown`: the owners'
    /// outcomes cannot be compared (model_reasoning_metadata.rs:97 folds every failure into
    /// `Unknown`, a D1 finding), so the owned check is incomplete, and the case names that gap.
    InjectedAmbiguous,
}

#[expect(
    clippy::expect_used,
    reason = "test fixture: a registry naming a contract the checker lacks must fail the calling tap"
)]
fn tap(case_name: &str, row: &str, raw: &Value, origin: BodyOrigin, accepted: Value) -> TapCase {
    let mut case = TapCase::new(ProtocolNode::N12ClaudeMetadata, case_name, row);
    case.use_owned_codecs(
        codex_router_test_support::node_taps::OwnedCodecs::from_owner_registry(
            crate::owned_contract_checks::round_trips(),
            crate::owned_contract_checks::verdicts(),
        )
        .expect("every listed contract is a Router contract"),
    );
    case.state_before(NodeStateSnapshot::new().field("decoder_state", "stateless"));
    // N12's own input (Claude SDK output inside the agent process) is not on a Router edge. What it
    // hands to N6 is the raw body, then what the Rust reader N6 uses accepted from it.
    case.produced_raw(
        model_reasoning_body(),
        MessageFamily::ClaudeModelMetadata,
        N6,
        RAW_KIND,
        raw.clone(),
        &[],
    );
    match origin {
        BodyOrigin::Producer => {}
        BodyOrigin::InjectedBothReject => case.mark_last(RecordIntent::DeliberateInvalid {
            keyword: "routerCodec",
            instance_path: "",
            detail: "bothReject",
        }),
        BodyOrigin::InjectedAmbiguous => case.declare_coverage_gap(
            RAW_KIND,
            "Zod accepts this body and the Rust decoder returns a fallback Unknown, which cannot say whether it accepted the value (model_reasoning_metadata.rs:97 folds every failure into Unknown, a D1 finding)",
        ),
    }
    case.produced_with(
        RecordLayer::TypedOutcome,
        MessageFamily::ClaudeModelMetadata,
        N6,
        KIND,
        accepted,
    );
    case.state_after(NodeStateSnapshot::new().field("decoder_state", "stateless"));
    case
}

fn expect_n6(raw: Value, accepted: Value) -> CatalogRowExpectation {
    CatalogRowExpectation::new()
        .produces_with(MessageFamily::ClaudeModelMetadata, N6, RAW_KIND, raw)
        .produces_with(MessageFamily::ClaudeModelMetadata, N6, KIND, accepted)
        .state_after(NodeStateSnapshot::new().field("decoder_state", "stateless"))
}

/// The producer's raw `modelReasoning` value for its scenario, written out independently.
fn expected_producer_body() -> Value {
    json!({
        "version": 1,
        "kind": "available",
        "models": [
            { "modelId": "sonnet", "kind": "select", "configId": "effort",
              "choices": [
                  { "value": "default", "label": "Default" },
                  { "value": "low", "label": "Low" },
                  { "value": "high", "label": "High" }
              ],
              "defaultValue": "default" },
            { "modelId": "haiku", "kind": "unknown", "reason": "Unavailable" }
        ]
    })
}

fn choice(value: &str, label: &str) -> ProviderSettingChoice {
    ProviderSettingChoice {
        value: value.to_owned(),
        label: label.to_owned(),
    }
}

/// The producer's scenario, written out independently: sonnet supports effort (low, high) with the
/// wrapper's labels ("Default" for the reset, then each level title-cased); haiku has no effort.
#[expect(
    clippy::expect_used,
    reason = "test literal: a literal the constructors refuse must fail the calling tap"
)]
fn expected_available() -> ModelReasoningMetadata {
    let sonnet = ModelReasoningSelect::try_new(
        "effort".to_owned(),
        vec![
            choice("default", "Default"),
            choice("low", "Low"),
            choice("high", "High"),
        ],
        "default".to_owned(),
    )
    .expect("literal select");
    ModelReasoningMetadata::Available {
        per_model_descriptors: vec![
            ModelReasoningEntry::try_new("sonnet".to_owned(), ModelReasoning::Select(sonnet))
                .expect("literal sonnet entry"),
            ModelReasoningEntry::try_new(
                "haiku".to_owned(),
                ModelReasoning::Unknown(ModelReasoningUnknown::Unavailable),
            )
            .expect("literal haiku entry"),
        ],
    }
}

#[test]
fn node_tap_v1_available_models_decode_with_select_and_unknown_rows() {
    let raw = produced_model_reasoning();
    let decoded = decode(&raw);
    let case = tap(
        "v1_available_models",
        "Claude model reasoning v1: available models (model id, config id, choices, default)",
        &raw,
        BodyOrigin::Producer,
        whole(&decoded),
    );
    case.finish(&expect_n6(
        expected_producer_body(),
        whole(&expected_available()),
    ))
    .assert_settled();
}

#[test]
fn node_tap_v1_unknown_with_reason_decodes_to_unknown() {
    // The real output's haiku entry: a model whose reasoning is unknown, with its reason.
    let raw = produced_model_reasoning();
    let decoded = decode(&raw);
    let case = tap(
        "v1_unknown_with_reason",
        "Claude model reasoning v1: unknown with a reason (UnsupportedSource ...)",
        &raw,
        BodyOrigin::Producer,
        whole(&decoded.for_model("haiku")),
    );
    case.finish(&expect_n6(
        expected_producer_body(),
        whole(&ModelReasoning::Unknown(ModelReasoningUnknown::Unavailable)),
    ))
    .assert_settled();
}

#[test]
fn node_tap_retained_difference_unknown_keys_are_rejected_by_rust() {
    // Zod would strip the extra key and accept; Rust rejects it (named, retained).
    let mut meta = produced_model_reasoning();
    meta["models"][0]["extraKey"] = json!("not in the contract");
    let case = tap(
        "retained_difference_unknown_key_rejected",
        "Retained difference: Zod strips unknown keys, Rust rejects them",
        &meta,
        BodyOrigin::InjectedAmbiguous,
        whole(&decode(&meta)),
    );
    let mut injected = expected_producer_body();
    injected["models"][0]["extraKey"] = json!("not in the contract");
    case.finish(&expect_n6(
        injected,
        whole(&ModelReasoningMetadata::Unknown(
            ModelReasoningUnknown::Malformed,
        )),
    ))
    .assert_settled();
}

#[test]
fn node_tap_retained_difference_other_versions_are_unrecognized() {
    // Zod pins version 1; Rust reports any other version as Unrecognized (named, retained).
    let mut meta = produced_model_reasoning();
    meta["version"] = json!(2);
    let case = tap(
        "retained_difference_other_version_unrecognized",
        "Retained difference: Zod pins version 1, Rust reports other versions as Unrecognized",
        &meta,
        BodyOrigin::InjectedBothReject,
        whole(&decode(&meta)),
    );
    let mut injected = expected_producer_body();
    injected["version"] = json!(2);
    case.finish(&expect_n6(
        injected,
        whole(&ModelReasoningMetadata::Unknown(
            ModelReasoningUnknown::Unrecognized,
        )),
    ))
    .assert_settled();
}
