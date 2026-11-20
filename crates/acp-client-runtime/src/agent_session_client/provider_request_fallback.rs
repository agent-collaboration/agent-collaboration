//! Connection-level answer for agent requests that no implemented handler owns.

use agent_client_protocol::schema::v1::RequestId;
use agent_client_protocol::{Agent, ConnectionTo, Dispatch, Error, HandleDispatchFrom, Handled};
use session_event_model::managed_interaction_refusal::ManagedInteractionRefusal;
use std::{
    collections::HashSet,
    hash::{BuildHasher as _, RandomState},
    sync::Arc,
};
use tokio::sync::RwLock;

#[derive(Clone, Default)]
pub(super) struct ProviderKnownSessions(Arc<RwLock<HashSet<String>>>);

impl ProviderKnownSessions {
    pub(super) async fn track(&self, session_id: String) {
        self.0.write().await.insert(session_id);
    }

    pub(super) async fn forget(&self, session_id: &str) {
        self.0.write().await.remove(session_id);
    }

    pub(super) async fn snapshot(&self) -> Vec<String> {
        self.0.read().await.iter().cloned().collect()
    }

    async fn contains(&self, session_id: &str) -> bool {
        self.0.read().await.contains(session_id)
    }
}

#[derive(Eq, Hash, PartialEq)]
struct CallbackIdentity {
    request_id: RequestId,
    method: String,
    params_digest: [u64; 2],
}

pub(super) struct ProviderRequestSessionGuard {
    known_sessions: ProviderKnownSessions,
    managed_refusal: bool,
    params_hashers: [RandomState; 2],
    claimed_callbacks: HashSet<CallbackIdentity>,
}

impl ProviderRequestSessionGuard {
    pub(super) fn new(known_sessions: ProviderKnownSessions, managed_refusal: bool) -> Self {
        Self {
            known_sessions,
            managed_refusal,
            params_hashers: std::array::from_fn(|_| RandomState::new()),
            claimed_callbacks: HashSet::new(),
        }
    }
}

impl HandleDispatchFrom<Agent> for ProviderRequestSessionGuard {
    async fn handle_dispatch_from(
        &mut self,
        message: Dispatch,
        _connection: ConnectionTo<Agent>,
    ) -> Result<Handled<Dispatch>, Error> {
        if let Dispatch::Request(request, responder) = message {
            // Retain claims through connection retirement so a late replay
            // cannot claim the next turn. A provider can reuse an ID.
            if self.managed_refusal
                && ManagedInteractionRefusal::owns_acp_callback(request.method())
            {
                let mut params = request.params().clone();
                params.sort_all_objects();
                let canonical_params = params.to_string();
                let identity = CallbackIdentity {
                    request_id: responder.id().clone(),
                    method: request.method().to_owned(),
                    params_digest: self
                        .params_hashers
                        .each_ref()
                        .map(|hasher| hasher.hash_one(&canonical_params)),
                };
                if !self.claimed_callbacks.insert(identity) {
                    return Ok(Handled::Yes);
                }
            }
            let session_id = request
                .params()
                .get("sessionId")
                .and_then(serde_json::Value::as_str);
            if let Some(session_id) = session_id
                && !self.known_sessions.contains(session_id).await
            {
                responder.respond_with_error(Error::method_not_found())?;
                return Ok(Handled::Yes);
            }
            return Ok(Handled::No {
                message: Dispatch::Request(request, responder),
                retry: false,
            });
        }
        Ok(Handled::No {
            message,
            retry: false,
        })
    }

    fn describe_chain(&self) -> impl std::fmt::Debug {
        "ProviderRequestSessionGuard"
    }
}

pub(super) struct ProviderRequestFallback;

impl HandleDispatchFrom<Agent> for ProviderRequestFallback {
    async fn handle_dispatch_from(
        &mut self,
        message: Dispatch,
        _connection: ConnectionTo<Agent>,
    ) -> Result<Handled<Dispatch>, Error> {
        match message {
            Dispatch::Request(_, responder) => {
                responder.respond_with_error(Error::method_not_found())?;
                Ok(Handled::Yes)
            }
            Dispatch::Notification(_) | Dispatch::Response(_, _) => Ok(Handled::No {
                message,
                retry: false,
            }),
        }
    }

    fn describe_chain(&self) -> impl std::fmt::Debug {
        "ProviderRequestFallback"
    }
}
