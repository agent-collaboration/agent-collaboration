//! Resolve an endpoint-free central address against local session records.
use claude_code_peer_messaging::{ClaudeCodeSessionRegistry, PeerSessionLookup};
use collaboration_protocol::{EndpointRef, HarnessSessionRef};
use collaboration_runtime::{
    CentralRecipientLookup, CentralRecipientUnavailable, EndpointDirectory, ProviderOperationStore,
    TransportFuture,
};
use std::{path::PathBuf, sync::Arc};
use tokio::sync::Mutex;
use workspace_ids::{MachineId, SessionRef};
use workspace_model::central_mail::{
    DeliveryAnswer, MessageBody, MessageRecord, MessageRefusal, MessageSender,
};

pub(super) struct CentralRecipientDirectory {
    pub(super) machine_id: MachineId,
    pub(super) endpoints: EndpointDirectory,
    pub(super) codex_home: PathBuf,
    pub(super) provider_store: Option<Arc<Mutex<ProviderOperationStore>>>,
    pub(super) claude_sessions: Arc<ClaudeCodeSessionRegistry>,
}

enum RecipientMatch {
    Unknown,
    Ambiguous,
    Known(EndpointRef),
}

impl CentralRecipientDirectory {
    async fn resolve(
        &self,
        address: &SessionRef,
    ) -> Result<RecipientMatch, CentralRecipientUnavailable> {
        if address.machine_id != self.machine_id {
            return Ok(RecipientMatch::Unknown);
        }
        let session_id =
            collaboration_protocol::SessionId::try_from(address.session_id.as_str().to_owned())
                .map_err(|_| CentralRecipientUnavailable)?;
        let endpoints = self
            .endpoints
            .inventory()
            .map_err(|_| CentralRecipientUnavailable)?;
        let mut matched = None;
        for item in endpoints.endpoints {
            let target = HarnessSessionRef {
                endpoint: item.endpoint,
                session_id: session_id.clone(),
            };
            if self.contains_session(&target).await? && matched.replace(target.endpoint).is_some() {
                return Ok(RecipientMatch::Ambiguous);
            }
        }
        Ok(matched.map_or(RecipientMatch::Unknown, RecipientMatch::Known))
    }

    async fn contains_session(
        &self,
        target: &HarnessSessionRef,
    ) -> Result<bool, CentralRecipientUnavailable> {
        let endpoint_id = String::from(target.endpoint.endpoint_id.clone());
        if endpoint_id == "codex-local" {
            let exists = tokio::fs::try_exists(self.codex_home.join("state_5.sqlite"))
                .await
                .map_err(|_| CentralRecipientUnavailable)?;
            if !exists {
                return Ok(false);
            }
            let catalog = codex_native_integration::StoredThreadCatalog::open(&self.codex_home)
                .await
                .map_err(|_| CentralRecipientUnavailable)?;
            return catalog
                .contains_session(&String::from(target.session_id.clone()))
                .await
                .map_err(|_| CentralRecipientUnavailable);
        }
        if endpoint_id == "claude-local" {
            let registry = self.claude_sessions.clone();
            let session_id = target.session_id.clone();
            let live = tokio::task::spawn_blocking(move || registry.lookup(&session_id))
                .await
                .map_err(|_| CentralRecipientUnavailable)?
                .map_err(|_| CentralRecipientUnavailable)?;
            if !matches!(live, PeerSessionLookup::Absent) {
                return Ok(true);
            }
        }
        let store = self
            .provider_store
            .as_ref()
            .ok_or(CentralRecipientUnavailable)?;
        Ok(store
            .lock()
            .await
            .session_record(target)
            .await
            .map_err(|_| CentralRecipientUnavailable)?
            .is_some())
    }
}

impl CentralRecipientLookup for CentralRecipientDirectory {
    fn answer<'call>(
        &'call self,
        message: &'call MessageRecord,
    ) -> TransportFuture<'call, Result<DeliveryAnswer, CentralRecipientUnavailable>> {
        Box::pin(async move {
            if message.target.machine_id == self.machine_id
                && message.sender == MessageSender::Central
                && matches!(message.body, MessageBody::DeliveryOutcome { .. })
            {
                // Request correction uses the outbox even if the sender session has exited.
                return Ok(DeliveryAnswer::Accepted);
            }
            let answer = match self.resolve(&message.target).await? {
                RecipientMatch::Known(endpoint) => {
                    tracing::debug!(endpoint_id = %String::from(endpoint.endpoint_id), "central inbox recipient resolved");
                    DeliveryAnswer::Accepted
                }
                RecipientMatch::Unknown => {
                    tracing::debug!("central inbox refused unknown local session");
                    DeliveryAnswer::Refused {
                        reason: MessageRefusal::UnknownSession,
                    }
                }
                RecipientMatch::Ambiguous => {
                    tracing::debug!(
                        "central inbox refused a session present on multiple endpoints"
                    );
                    DeliveryAnswer::Refused {
                        reason: MessageRefusal::AmbiguousSession,
                    }
                }
            };
            Ok(answer)
        })
    }
}

#[cfg(test)]
#[path = "central_recipient_directory_test.rs"]
mod tests;
