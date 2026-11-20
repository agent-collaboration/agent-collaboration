//! A tool call that holds one request slot until the test releases it, for listener admission
//! tests in this crate and in the client and CLI crates.
//!
//! The holding call is a real `events_observe` on a recorded provider Session: it waits on the
//! Session hub and ends at its first event, so releasing it is publishing one event, not
//! waiting for a timer.
use collaboration_protocol::{
    EndpointDescription, ProviderRequestedPolicy, ProviderWorkingDirectory, RouterAccess,
    SessionRef,
};
use collaboration_runtime::{
    ProviderOperationStore, ProviderSessionEventHub, ProviderSessionRecord,
};
use collaboration_service::ApplicationFixture;
use serde_json::{Value, json};
use std::{io, path::Path, sync::Arc};
use tokio::sync::Mutex;

/// The provider Session whose observation holds the slot, and the hub that releases it.
pub struct RequestSlotHolder {
    hub: Arc<ProviderSessionEventHub>,
    target: SessionRef,
}

impl RequestSlotHolder {
    /// Gives `identity`, whose service is `service_id`, a provider endpoint with one recorded
    /// Session to observe. Its provider operation store lives in `directory`.
    pub async fn install(
        identity: ApplicationFixture,
        service_id: &str,
        directory: &Path,
    ) -> io::Result<(ApplicationFixture, Self)> {
        let endpoint = json!({"serviceId": service_id, "endpointId": "claude-local"});
        let target: SessionRef =
            serde_json::from_value(json!({"endpoint": endpoint, "sessionId": "slot-holder"}))
                .map_err(io::Error::other)?;
        let owner: SessionRef = serde_json::from_value(json!({
            "endpoint": {"serviceId": service_id, "endpointId": "codex-local"},
            "sessionId": "slot-holder-owner"
        }))
        .map_err(io::Error::other)?;
        let store = ProviderOperationStore::open(&directory.join("slot-holder-operations.sqlite"))
            .await
            .map_err(io::Error::other)?;
        let store = Arc::new(Mutex::new(store));
        store
            .lock()
            .await
            .record_session(&ProviderSessionRecord {
                target: target.clone(),
                working_directory: ProviderWorkingDirectory::try_from(
                    directory.display().to_string(),
                )
                .map_err(io::Error::other)?,
                requested_policy: ProviderRequestedPolicy {
                    access: RouterAccess::WriteRestricted,
                },
                created_by: owner.clone().into(),
                approver: owner.into(),
                updated_at_ms: 1,
            })
            .await
            .map_err(io::Error::other)?;
        let hub = Arc::new(ProviderSessionEventHub::with_capacity(store, 16));
        let identity = identity.with_provider_session_hub(
            Arc::clone(&hub) as Arc<dyn collaboration_contracts::SessionEventHub>
        );
        let description: EndpointDescription = serde_json::from_value(json!({
            "endpoint": endpoint,
            "label": "Slot holder",
            "availability": {"state": "available", "observedAt": "2026-09-26T00:00:00Z"},
            "channels": [{"kind": "externalProvider", "transport": "stdioAcp",
                "bindingId": "slot-holder-binding", "bindingGeneration": 1,
                "runtime": {"provider": "claudeCode", "runtimeName": "fixture"},
                "capabilities": [{"name": "create", "status": "supported", "evidence": "advertised"}]}]
        }))
        .map_err(io::Error::other)?;
        identity
            .endpoint_directory()
            .publish(description)
            .map_err(io::Error::other)?;
        Ok((identity, Self { hub, target }))
    }

    /// The tool that holds a slot until [`Self::release`].
    pub const TOOL: &'static str = "events_observe";

    /// Arguments for [`Self::TOOL`]: wait for one event, bounded far beyond any test.
    #[must_use]
    pub fn arguments(&self) -> Value {
        json!({
            "target": self.target, "timeoutSeconds": 300, "maxEvents": 1, "maxBytes": 65_536
        })
    }

    /// Ends the holding call by publishing the one event it waits for.
    pub async fn release(&self) -> io::Result<()> {
        let session =
            serde_json::from_value(serde_json::to_value(&self.target).map_err(io::Error::other)?)
                .map_err(io::Error::other)?;
        let event = serde_json::from_value(json!({
            "kind": "itemStarted",
            "item": {"itemId": "slot-release", "kind": {"kind": "agentMessage"}, "text": "release"}
        }))
        .map_err(io::Error::other)?;
        self.hub
            .publish(session, event)
            .await
            .map(|_| ())
            .map_err(io::Error::other)
    }
}
