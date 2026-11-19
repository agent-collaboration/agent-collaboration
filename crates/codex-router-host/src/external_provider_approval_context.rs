//! Who a provider prompt acts for and who approves what it asks: the context
//! each prompt carries into the agent's permission requests, and that a steer
//! returns for the turn it joined (design note §12.4). A leaf: it depends on
//! nothing else in the Host.

use collaboration_protocol::{CodexGeneration, OperationId, ProviderIdentity, SessionRef};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug)]
pub struct ExternalProviderApprovalContext {
    pub requester: ProviderIdentity,
    pub approver: ProviderIdentity,
    pub target: SessionRef,
    pub operation_id: OperationId,
    pub binding_generation: CodexGeneration,
    pub binding_retirement: CancellationToken,
}
