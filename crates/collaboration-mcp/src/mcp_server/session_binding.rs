//! Resolve each call's binding before dispatch; operator actions retain their existing policy.
use super::{CallToolResult, CollaborationMcpServer, structured_tool_error};
use crate::collaboration_api_router::CallTransport;
use collaboration_service::fleet_session_registrar::{BoundFleetSession, SessionRegistrationMode};
use message_board::{Identity, ReadActor};

#[derive(Clone)]
pub(super) enum CallSessionAdmission {
    AccessEnrollment,
    BoundHttp(BoundFleetSession),
    Unbound(SessionContextAdmission),
}

#[derive(Clone)]
pub(super) enum SessionContextAdmission {
    Missing,
    Inactive,
    Active(BoundFleetSession),
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
enum BindingRefusal {
    SenderMismatch,
    SessionContextRequired,
    InactiveSessionBinding,
}

fn refusal(kind: BindingRefusal) -> CallToolResult {
    let message = match kind {
        BindingRefusal::SenderMismatch => "Declared sender differs from the calling session",
        BindingRefusal::SessionContextRequired => {
            "A session mutation requires its current session context"
        }
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

fn owner_unix_context(
    parts: Option<&http::request::Parts>,
) -> Option<collaboration_protocol::UnixSessionContext> {
    let parts = parts.filter(|parts| {
        matches!(
            parts.extensions.get::<CallTransport>(),
            Some(CallTransport::OwnerUnix)
        )
    })?;
    let mut values = parts
        .headers
        .get_all(collaboration_protocol::UNIX_SESSION_CONTEXT_HEADER)
        .iter();
    let value = values.next()?;
    if values.next().is_some() {
        return None;
    }
    serde_json::from_slice(value.as_bytes()).ok()
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
        let claim = owner_unix_context(parts);
        let admission = match claim {
            None => SessionContextAdmission::Missing,
            Some(claim) => {
                match collaboration_protocol::ProviderIdentity::Session(claim.session)
                    .to_board_identity()
                {
                    Ok(Identity::Session { session }) => {
                        match registrar.resolve_session_context(&session).await {
                            Ok(bound) => SessionContextAdmission::Active(bound),
                            Err(_) => SessionContextAdmission::Inactive,
                        }
                    }
                    Ok(Identity::Human { .. }) | Err(_) => SessionContextAdmission::Inactive,
                }
            }
        };
        server.call_session = CallSessionAdmission::Unbound(admission);
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
            CallSessionAdmission::Unbound(_) if matches!(actor, Identity::Human { .. }) => None,
            CallSessionAdmission::Unbound(SessionContextAdmission::Active(bound)) => bound
                .require_actor(actor)
                .err()
                .map(|_| refusal(BindingRefusal::SenderMismatch)),
            CallSessionAdmission::Unbound(SessionContextAdmission::Missing) => {
                Some(refusal(BindingRefusal::SessionContextRequired))
            }
            CallSessionAdmission::Unbound(SessionContextAdmission::Inactive) => {
                Some(refusal(BindingRefusal::InactiveSessionBinding))
            }
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

    pub(super) fn creator_claim_refusal(
        &self,
        actor: &collaboration_client::ConversationCreateActor,
    ) -> Option<CallToolResult> {
        match actor {
            collaboration_client::ConversationCreateActor::Session(session) => {
                self.sender_claim_refusal(session)
            }
            collaboration_client::ConversationCreateActor::Typed(actor) => {
                self.actor_claim_refusal(actor)
            }
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
    use super::{CallSessionAdmission, CallTransport, SessionContextAdmission, owner_unix_context};
    use crate::mcp_server::CollaborationMcpServer;
    use message_board::{Identity, ReadActor};
    use serde_json::{Value, json};

    fn session_claim() -> Identity {
        serde_json::from_value(json!({"kind":"session","session":{
            "endpoint":{"serviceId":"019f0000-0000-7000-8000-000000000003","endpointId":"codex-local"},
            "sessionId":"fixture-current-session"
        }})).unwrap()
    }
    fn operator_claim() -> Identity {
        serde_json::from_value(json!({"kind":"human","humanId":"fixture-operator"})).unwrap()
    }
    fn refusal_kind(server: &CollaborationMcpServer, actor: &Identity) -> Option<Value> {
        server
            .actor_claim_refusal(actor)
            .and_then(|result| result.structured_content)
            .and_then(|result| result.get("kind").cloned())
    }
    fn request_parts(transport: CallTransport, value: &str) -> http::request::Parts {
        let mut request = http::Request::new(());
        request.extensions_mut().insert(transport);
        request.headers_mut().insert(
            collaboration_protocol::UNIX_SESSION_CONTEXT_HEADER,
            http::HeaderValue::from_bytes(value.as_bytes()).unwrap(),
        );
        request.into_parts().0
    }

    #[test]
    fn session_binding_missing_and_inactive_context_refuse_session_mutations_and_keep_operator_actions()
     {
        let mut server = CollaborationMcpServer::catalog_only();
        let actor = session_claim();
        assert_eq!(refusal_kind(&server, &actor), None);
        assert_eq!(refusal_kind(&server, &operator_claim()), None);

        server.call_session = CallSessionAdmission::Unbound(SessionContextAdmission::Missing);
        assert_eq!(
            refusal_kind(&server, &actor),
            Some(json!("sessionContextRequired"))
        );
        assert_eq!(refusal_kind(&server, &operator_claim()), None);
        assert!(
            server
                .actor_claim_refusal(&ReadActor::Anonymous {})
                .is_none()
        );

        server.call_session = CallSessionAdmission::Unbound(SessionContextAdmission::Inactive);
        assert_eq!(
            refusal_kind(&server, &actor),
            Some(json!("inactiveSessionBinding"))
        );
        assert_eq!(refusal_kind(&server, &operator_claim()), None);
    }

    #[test]
    fn session_binding_context_header_is_accepted_only_on_the_owner_unix_transport() {
        let Identity::Session { session } = session_claim() else {
            panic!("fixture session");
        };
        let claim: collaboration_protocol::UnixSessionContext =
            serde_json::from_value(json!({"session":session})).unwrap();
        let header = serde_json::to_string(&claim).unwrap();
        let parts = request_parts(CallTransport::OwnerUnix, &header);
        assert_eq!(owner_unix_context(Some(&parts)), Some(claim));
        let parts = request_parts(CallTransport::Http, &header);
        assert_eq!(owner_unix_context(Some(&parts)), None);

        let mut parts = request_parts(CallTransport::OwnerUnix, &header);
        parts.headers.append(
            collaboration_protocol::UNIX_SESSION_CONTEXT_HEADER,
            http::HeaderValue::from_static("{}"),
        );
        assert_eq!(owner_unix_context(Some(&parts)), None);
        for malformed in [
            "{}",
            "null",
            r#"{"session":{},"capability":"not-an-export"}"#,
        ] {
            let parts = request_parts(CallTransport::OwnerUnix, malformed);
            assert_eq!(owner_unix_context(Some(&parts)), None);
        }
    }
}
