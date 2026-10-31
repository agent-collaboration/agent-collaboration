//! Test-only clock input at the existing Host composition owner.
use super::{CollaborationRuntime, CollaborationRuntimeInputs, RouterExecutableRelation};
use std::{io, sync::Arc};

impl CollaborationRuntime {
    pub async fn runtime_test_start_with_subscription_clock(
        inputs: CollaborationRuntimeInputs,
        clock: Arc<dyn collaboration_service::SubscriptionClock>,
    ) -> io::Result<Self> {
        let (_relation_sender, relation) =
            tokio::sync::watch::channel(RouterExecutableRelation::Match);
        Box::pin(Self::start_with_optional_router_proxy_endpoint(
            inputs,
            Vec::new(),
            relation,
            None,
            None,
            clock,
        ))
        .await
    }

    pub fn runtime_test_observe_wake_progress(
        &self,
    ) -> io::Result<tokio::sync::watch::Receiver<usize>> {
        self.runtime_test_wake_progress
            .clone()
            .ok_or_else(|| io::Error::other("test wake worker is unavailable"))
    }
}
