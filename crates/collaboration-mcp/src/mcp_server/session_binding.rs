//! Resolve each call's binding before dispatch; operator actions retain their existing policy.
use super::{CallToolResult, CollaborationMcpServer, structured_tool_error};
use collaboration_service::fleet_session_registrar::{BoundFleetSession, SessionRegistrationMode};
use message_board::{Identity, ReadActor};

#[derive(Clone)]
pub(super) enum CallSessionAdmission {
    AccessEnrollment,
    BoundHttp(BoundFleetSession),
    Unbound,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
enum BindingRefusal {
    SenderMismatch,
    SessionBindingRequired,
    InactiveSessionBinding,
}

fn refusal(kind: BindingRefusal) -> CallToolResult {
    let message = match kind {
        BindingRefusal::SenderMismatch => "Declared sender differs from the calling session",
        BindingRefusal::SessionBindingRequired => "A session mutation requires its bound address",
        BindingRefusal::InactiveSessionBinding => {
            "The calling session binding is inactive or unavailable"
        }
    };
    structured_tool_error(serde_json::json!({
        "kind": kind, "stage":"admission", "effect":"none", "message":message
    }))
}

pub(super) trait CallActorClaim {
    fn claimed_identity(&self) -> Option<&Identity>;
}
impl CallActorClaim for Identity {
    fn claimed_identity(&self) -> Option<&Identity> {
        Some(self)
    }
}
impl CallActorClaim for ReadActor {
    fn claimed_identity(&self) -> Option<&Identity> {
        self.identity()
    }
}

impl CollaborationMcpServer {
    pub(super) async fn admit_call_session(
        &self,
        context: &rmcp::service::RequestContext<rmcp::service::RoleServer>,
    ) -> Result<Self, Box<CallToolResult>> {
        let mut server = self.clone();
        let parts = context.extensions.get::<http::request::Parts>();
        let capability = parts.and_then(|parts| parts.uri.path().strip_prefix("/mcp/session/"));
        if let Some(capability) = capability {
            let Some(registrar) = &self.session_registrar else {
                return Err(Box::new(refusal(BindingRefusal::InactiveSessionBinding)));
            };
            let bound = registrar
                .resolve_binding_capability(capability)
                .await
                .map_err(|_| Box::new(refusal(BindingRefusal::InactiveSessionBinding)))?;
            server.call_session = CallSessionAdmission::BoundHttp(bound);
            return Ok(server);
        }
        let Some(registrar) = &self.session_registrar else {
            return Ok(server);
        };
        if registrar.registration_mode() == SessionRegistrationMode::AccessEnrollment {
            return Ok(server);
        }
        server.call_session = CallSessionAdmission::Unbound;
        Ok(server)
    }

    pub(super) fn actor_claim_refusal(
        &self,
        claim: &impl CallActorClaim,
    ) -> Option<CallToolResult> {
        let actor = claim.claimed_identity()?;
        match &self.call_session {
            CallSessionAdmission::AccessEnrollment => None,
            CallSessionAdmission::BoundHttp(bound) => bound
                .require_actor(actor)
                .err()
                .map(|_| refusal(BindingRefusal::SenderMismatch)),
            CallSessionAdmission::Unbound => match actor {
                Identity::Human { .. } => None,
                Identity::Session { .. } => Some(refusal(BindingRefusal::SessionBindingRequired)),
            },
        }
    }

    pub(super) fn sender_claim_refusal(
        &self,
        sender: &collaboration_protocol::SessionRef,
    ) -> Option<CallToolResult> {
        if matches!(self.call_session, CallSessionAdmission::AccessEnrollment) {
            return None;
        }
        match collaboration_protocol::ProviderIdentity::Session(sender.clone()).to_board_identity()
        {
            Ok(actor) => self.actor_claim_refusal(&actor),
            Err(_) => Some(refusal(BindingRefusal::SenderMismatch)),
        }
    }

    pub(super) fn provider_actor_claim_refusal(
        &self,
        actor: &collaboration_protocol::ProviderIdentity,
    ) -> Option<CallToolResult> {
        if matches!(self.call_session, CallSessionAdmission::AccessEnrollment) {
            return None;
        }
        match actor.to_board_identity() {
            Ok(actor) => self.actor_claim_refusal(&actor),
            Err(_) => Some(refusal(BindingRefusal::SenderMismatch)),
        }
    }

    pub(super) fn prompt_author_claim_refusal(
        &self,
        prompt: &collaboration_client::PublicPromptContent,
    ) -> Option<CallToolResult> {
        match prompt {
            collaboration_client::PublicPromptContent::Agent { sender, .. } => {
                self.sender_claim_refusal(sender)
            }
            collaboration_client::PublicPromptContent::HumanUser { .. } => {
                self.operator_claim_refusal()
            }
        }
    }

    pub(super) fn operator_claim_refusal(&self) -> Option<CallToolResult> {
        matches!(self.call_session, CallSessionAdmission::BoundHttp(_))
            .then(|| refusal(BindingRefusal::SenderMismatch))
    }

    pub(super) fn wake_request_claim_refusal(
        &self,
        request: &collaboration_protocol::WakeSendRequest,
    ) -> Option<CallToolResult> {
        if matches!(self.call_session, CallSessionAdmission::AccessEnrollment) {
            return None;
        }
        let creator_refusal = match &request.requested_by {
            collaboration_protocol::WakeCreator::Session(session) => {
                self.sender_claim_refusal(session)
            }
            collaboration_protocol::WakeCreator::Owner {} => self.operator_claim_refusal(),
            collaboration_protocol::WakeCreator::Router {}
            | collaboration_protocol::WakeCreator::Unrecorded {} => {
                Some(refusal(BindingRefusal::SenderMismatch))
            }
        };
        creator_refusal.or_else(|| match &request.message.content {
            collaboration_protocol::MessageContent::Agent { sender, .. } => {
                self.sender_claim_refusal(sender)
            }
            collaboration_protocol::MessageContent::HumanUser { .. } => {
                self.operator_claim_refusal()
            }
            collaboration_protocol::MessageContent::Router { .. } => {
                Some(refusal(BindingRefusal::SenderMismatch))
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::CallSessionAdmission;
    use crate::mcp_server::CollaborationMcpServer;
    use message_board::{Identity, ReadActor};
    use serde_json::json;

    #[test]
    fn session_binding_unbound_calls_refuse_every_session_sender_and_keep_operator_actions() {
        let mut server = CollaborationMcpServer::catalog_only();
        let operator: Identity =
            serde_json::from_value(json!({"kind":"human","humanId":"fixture-operator"})).unwrap();
        for session_id in ["current-A", "claimed-B"] {
            let actor: Identity = serde_json::from_value(json!({"kind":"session","session":{
                "endpoint":{"serviceId":"019f0000-0000-7000-8000-000000000003","endpointId":"codex-local"},
                "sessionId":session_id
            }})).unwrap();
            server.call_session = CallSessionAdmission::AccessEnrollment;
            assert!(server.actor_claim_refusal(&actor).is_none());
            server.call_session = CallSessionAdmission::Unbound;
            let refusal = server
                .actor_claim_refusal(&actor)
                .unwrap()
                .structured_content
                .unwrap();
            assert_eq!(refusal["kind"], "sessionBindingRequired");
            assert_eq!(refusal["effect"], "none");
            assert!(server.actor_claim_refusal(&operator).is_none());
        }
        assert!(
            server
                .actor_claim_refusal(&ReadActor::Anonymous {})
                .is_none()
        );
    }
}
