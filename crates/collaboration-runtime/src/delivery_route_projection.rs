//! Convert persisted route evidence to the public inspection shape.
use agent_automation::RouteEffectEvidence;
use collaboration_protocol::{
    CodexGeneration, DeliveryRouteEvidence, OperationId, ProviderBindingId, SessionRef,
};

#[expect(
    clippy::result_unit_err,
    reason = "The extraction preserves the existing projection error boundary."
)]
pub fn project(
    evidence: &RouteEffectEvidence<SessionRef, CodexGeneration>,
) -> Result<DeliveryRouteEvidence, ()> {
    match evidence {
        RouteEffectEvidence::CodexAppServer(native) => Ok(DeliveryRouteEvidence::CodexAppServer(
            serde_json::from_value(serde_json::to_value(native).map_err(|_| ())?)
                .map_err(|_| ())?,
        )),
        RouteEffectEvidence::ProviderAcp(provider) => Ok(DeliveryRouteEvidence::ProviderAcp {
            binding_id: ProviderBindingId::try_from(provider.binding.as_str().to_owned())
                .map_err(|_| ())?,
            generation: provider.generation.clone(),
            target: provider.target.clone(),
            operation_id: OperationId::try_from(provider.attempt_id.as_str().to_owned())
                .map_err(|_| ())?,
            submission: serde_json::from_value(
                serde_json::to_value(provider.submission).map_err(|_| ())?,
            )
            .map_err(|_| ())?,
        }),
        RouteEffectEvidence::ClaudeCodePeer(peer) => Ok(DeliveryRouteEvidence::ClaudeCodePeer {
            session_id: peer.session_id.clone(),
            write: peer.write,
        }),
    }
}
