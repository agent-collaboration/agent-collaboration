//! Select the owned invocation tag without interpreting or changing its request.
use crate::DispatchCallSelector;
use fleet_sync_protocol::{ItemId, PeerId};
use serde::Deserialize;

#[derive(Clone)]
pub(super) enum DispatchSelection {
    Any,
    Call(DispatchCallSelector),
    ExactRequest(Vec<u8>),
}

#[derive(Deserialize)]
struct InvocationKind {
    kind: SelectedInvocationKind,
}

#[derive(Deserialize)]
struct FleetInvocation {
    kind: SelectedInvocationKind,
    request: FleetSubmitIdentity,
}

#[derive(Deserialize)]
struct FleetSubmitIdentity {
    session: SubmittingSession,
    item: FleetItem,
}

#[derive(Deserialize)]
struct SubmittingSession {
    peer: PeerId,
}

#[derive(Deserialize)]
struct FleetItem {
    value: FleetItemIdentity,
}

#[derive(Deserialize)]
struct FleetItemIdentity {
    id: ItemId,
}

#[derive(Deserialize)]
enum SelectedInvocationKind {
    #[serde(rename = "central.mail-submit")]
    FleetMailSubmit,
    #[serde(rename = "central.installation-peer")]
    InstallationPeer,
    #[serde(rename = "central.forward-route")]
    ForwardRoute,
    #[serde(rename = "central.machine-notice")]
    FleetMachineNotice,
    #[serde(rename = "fleet-sync.claim-settlement")]
    FleetClaimSettlement,
    #[serde(rename = "central.copy-session-write")]
    CopyWrite,
    #[serde(rename = "fleet-sync.copy-fence")]
    CopyFence,
    #[serde(other)]
    Other,
}

impl DispatchSelection {
    pub(super) fn matches(&self, request: &[u8]) -> bool {
        match self {
            Self::Any | Self::Call(DispatchCallSelector::Any) => true,
            Self::ExactRequest(expected) => expected == request,
            Self::Call(DispatchCallSelector::FleetMailSubmitFor { peer, item }) => {
                serde_json::from_slice::<FleetInvocation>(request).is_ok_and(|invocation| {
                    matches!(invocation.kind, SelectedInvocationKind::FleetMailSubmit)
                        && invocation.request.session.peer == *peer
                        && invocation.request.item.value.id == *item
                })
            }
            Self::Call(call) => {
                serde_json::from_slice::<InvocationKind>(request).is_ok_and(|invocation| {
                    matches!(
                        (call, invocation.kind),
                        (
                            DispatchCallSelector::FleetMailSubmit,
                            SelectedInvocationKind::FleetMailSubmit
                        ) | (
                            DispatchCallSelector::InstallationPeer,
                            SelectedInvocationKind::InstallationPeer
                        ) | (
                            DispatchCallSelector::ForwardRoute,
                            SelectedInvocationKind::ForwardRoute
                        ) | (
                            DispatchCallSelector::FleetMachineNotice,
                            SelectedInvocationKind::FleetMachineNotice
                        ) | (
                            DispatchCallSelector::FleetClaimSettlement,
                            SelectedInvocationKind::FleetClaimSettlement
                        ) | (
                            DispatchCallSelector::CopyWrite,
                            SelectedInvocationKind::CopyWrite
                        ) | (
                            DispatchCallSelector::CopyFence,
                            SelectedInvocationKind::CopyFence
                        )
                    )
                })
            }
        }
    }
}
