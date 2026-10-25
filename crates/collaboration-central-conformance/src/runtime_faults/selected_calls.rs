//! Select the owned invocation tag without interpreting or changing its request.
use crate::DispatchCallSelector;
use serde::Deserialize;

#[derive(Clone, Copy)]
pub(super) enum DispatchSelection {
    Any,
    Call(DispatchCallSelector),
}

#[derive(Deserialize)]
struct InvocationKind {
    kind: SelectedInvocationKind,
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
    pub(super) fn matches(self, request: &[u8]) -> bool {
        match self {
            Self::Any => true,
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
