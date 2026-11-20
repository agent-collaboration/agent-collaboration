//! Capture the connection-scoped Claude auth update without private fields.

use std::sync::Arc;

use agent_client_protocol::{Agent, ConnectionTo, Dispatch, Error, HandleDispatchFrom, Handled};
use session_event_model::ProviderAuthStatus;
use session_event_model::session_profile_codec::decode_connection_auth_status;
use tokio::sync::RwLock;

use crate::{SessionEventSink, owner_wire::OwnerAcpWire};

pub(super) struct ProviderAuthStatusHandler {
    auth_status: Arc<RwLock<ProviderAuthStatus>>,
    wire: Arc<OwnerAcpWire>,
    event_sink: Arc<dyn SessionEventSink>,
}

impl ProviderAuthStatusHandler {
    pub(super) fn new(
        auth_status: Arc<RwLock<ProviderAuthStatus>>,
        wire: Arc<OwnerAcpWire>,
        event_sink: Arc<dyn SessionEventSink>,
    ) -> Self {
        Self {
            auth_status,
            wire,
            event_sink,
        }
    }
}

impl HandleDispatchFrom<Agent> for ProviderAuthStatusHandler {
    async fn handle_dispatch_from(
        &mut self,
        message: Dispatch,
        _connection: ConnectionTo<Agent>,
    ) -> Result<Handled<Dispatch>, Error> {
        let Dispatch::Notification(notification) = message else {
            return Ok(Handled::No {
                message,
                retry: false,
            });
        };
        if notification.method() != "_auth/status_update" {
            return Ok(Handled::No {
                message: Dispatch::Notification(notification),
                retry: false,
            });
        }
        let status = match decode_connection_auth_status(notification.params().clone()) {
            Ok(status) => status,
            Err(_) => {
                tracing::warn!("malformed provider auth status update");
                return Ok(Handled::Yes);
            }
        };
        *self.auth_status.write().await = status.clone();
        // Connection-wide: the agent's auth status belongs to the connection,
        // so it reaches every session's report, whatever its run.
        for session_id in self.wire.routed_sessions() {
            if self
                .event_sink
                .publish_connection_auth(&session_id, status.clone())
                .is_err()
            {
                tracing::debug!("a session did not accept its auth status update");
            }
        }
        Ok(Handled::Yes)
    }

    fn describe_chain(&self) -> impl std::fmt::Debug {
        "ProviderAuthStatusHandler"
    }
}
