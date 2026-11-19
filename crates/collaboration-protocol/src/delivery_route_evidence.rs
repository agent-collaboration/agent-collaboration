//! Public inspection projects each route's stored evidence without native assumptions.
use crate::{
    CodexGeneration, HarnessSessionRef, NativeEffectEvidence, OperationId, ProviderBindingId,
    SubmissionEffect,
};
use agent_automation::{PeerSessionReference, PeerWriteEffect};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Captured provider reconciliation context, separate from a public delivery receipt.
pub type ProviderAcpRouteEvidence =
    agent_automation::ProviderAcpEffectEvidence<HarnessSessionRef, CodexGeneration>;

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum DeliveryRouteEvidence {
    CodexAppServer(NativeEffectEvidence),
    ProviderAcp {
        binding_id: ProviderBindingId,
        generation: CodexGeneration,
        target: HarnessSessionRef,
        operation_id: OperationId,
        submission: SubmissionEffect,
    },
    ClaudeCodePeer {
        session_id: PeerSessionReference,
        write: PeerWriteEffect,
    },
}
