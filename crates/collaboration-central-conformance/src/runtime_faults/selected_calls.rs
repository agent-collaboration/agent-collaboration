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
struct MailboxInvocation {
    kind: SelectedInvocationKind,
    request: MailboxSubmitIdentity,
}

#[derive(Deserialize)]
struct MailboxSubmitIdentity {
    origin: PeerId,
    item: MailboxItem,
}

#[derive(Deserialize)]
struct MailboxItem {
    value: MailboxItemIdentity,
}

#[derive(Deserialize)]
struct MailboxItemIdentity {
    id: ItemId,
}

#[derive(Deserialize)]
enum SelectedInvocationKind {
    #[serde(rename = "central.mailbox-submit")]
    MailboxSubmit,
    #[serde(rename = "central.installation-peer")]
    InstallationPeer,
    #[serde(rename = "central.forward-route")]
    ForwardRoute,
    #[serde(rename = "central.copy-session-write")]
    CopyWrite,
    #[serde(other)]
    Other,
}

impl DispatchSelection {
    pub(super) fn matches(&self, request: &[u8]) -> bool {
        match self {
            Self::Any => true,
            Self::ExactRequest(expected) => expected == request,
            Self::Call(DispatchCallSelector::MailboxSubmitFor { origin, item }) => {
                serde_json::from_slice::<MailboxInvocation>(request).is_ok_and(|invocation| {
                    matches!(invocation.kind, SelectedInvocationKind::MailboxSubmit)
                        && invocation.request.origin == *origin
                        && invocation.request.item.value.id == *item
                })
            }
            Self::Call(call) => {
                serde_json::from_slice::<InvocationKind>(request).is_ok_and(|invocation| {
                    matches!(
                        (call, invocation.kind),
                        (
                            DispatchCallSelector::MailboxSubmit,
                            SelectedInvocationKind::MailboxSubmit
                        ) | (
                            DispatchCallSelector::InstallationPeer,
                            SelectedInvocationKind::InstallationPeer
                        ) | (
                            DispatchCallSelector::ForwardRoute,
                            SelectedInvocationKind::ForwardRoute
                        ) | (
                            DispatchCallSelector::CopyWrite,
                            SelectedInvocationKind::CopyWrite
                        )
                    )
                })
            }
        }
    }
}
