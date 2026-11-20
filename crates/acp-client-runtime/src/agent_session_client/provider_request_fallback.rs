//! Connection-level answer for agent requests that no implemented handler owns.

use agent_client_protocol::{Agent, ConnectionTo, Dispatch, Error, HandleDispatchFrom, Handled};
use std::sync::Arc;

pub(super) struct ProviderRequestSessionGuard {
    wire: Arc<crate::owner_wire::OwnerAcpWire>,
}

impl ProviderRequestSessionGuard {
    pub(super) fn new(wire: Arc<crate::owner_wire::OwnerAcpWire>) -> Self {
        Self { wire }
    }
}

impl HandleDispatchFrom<Agent> for ProviderRequestSessionGuard {
    async fn handle_dispatch_from(
        &mut self,
        message: Dispatch,
        _connection: ConnectionTo<Agent>,
    ) -> Result<Handled<Dispatch>, Error> {
        if let Dispatch::Request(request, responder) = message {
            let session_id = request
                .params()
                .get("sessionId")
                .and_then(serde_json::Value::as_str);
            if let Some(session_id) = session_id
                && self.wire.route_for_session(session_id).is_none()
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
