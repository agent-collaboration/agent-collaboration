//! Public inspection projects each route's stored evidence without native assumptions.
use crate::{
    CodexGeneration, NativeEffectEvidence, OperationId, ProviderBindingId, SessionRef,
    SubmissionEffect,
};
use agent_automation::{PeerSessionReference, PeerWriteEffect};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Captured provider reconciliation context, separate from a public delivery receipt.
pub type ProviderAcpRouteEvidence =
    agent_automation::ProviderAcpEffectEvidence<SessionRef, CodexGeneration>;

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
        target: SessionRef,
        operation_id: OperationId,
        submission: SubmissionEffect,
    },
    ClaudeCodePeer {
        session_id: PeerSessionReference,
        write: PeerWriteEffect,
    },
}
