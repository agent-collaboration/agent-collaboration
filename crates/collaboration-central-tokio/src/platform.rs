//! Native key transport and cryptography; Access policy remains in the core.

use axum::{
    extract::{FromRequestParts, ws::WebSocketUpgrade},
    response::IntoResponse,
};
use collaboration_central_core::{
    AccessJwk, AdmissionOrigin, CentralObservation, CentralPlatform, CoreHost, KeyFetchFailure,
    RawJwks, SerializedObjectCall, SignatureValidity, UpgradeFailure, VerifyFailure,
};
use runtime_primitives::Clock;
use std::{
    future::{Future, ready},
    sync::Arc,
    time::Duration,
};

#[derive(Debug, thiserror::Error)]
pub enum TokioPlatformError {
    #[error("Access key source must be an HTTP(S) URL")]
    InvalidKeySource,
    #[error("HTTP client could not be constructed")]
    HttpClient(#[source] reqwest::Error),
    #[error(transparent)]
    Store(#[from] durable_steps::StoreError),
}

#[derive(Clone)]
pub struct TokioCentralPlatform {
    client: reqwest::Client,
    key_source: reqwest::Url,
    clock: Arc<dyn Clock>,
    observe: Arc<dyn Fn(CentralObservation) + Send + Sync>,
    objects: Arc<crate::native_objects::NativeObjects>,
    ids: runtime_primitives::UuidV7Ids,
}

impl TokioCentralPlatform {
    /// Conformance-only real import/seed; no production or HTTP route.
    #[cfg(feature = "conformance")]
    pub async fn conformance_import_board_snapshot(
        &self,
        owner: collaboration_central_core::OwnerIdentity,
        state: collaboration_central_core::ConformanceBoardState,
        pages: Vec<fleet_sync_protocol::SnapshotPage>,
        manifest: fleet_sync_protocol::SnapshotManifest,
    ) -> Result<fleet_sync_protocol::ReplicationPosition, durable_steps::StoreError> {
        self.objects
            .import_board_snapshot(owner, state, pages, manifest)
            .await
    }

    #[cfg(feature = "conformance")]
    pub(crate) async fn runtime_control(
        &self,
        request: collaboration_central_conformance::RuntimeControlRequest,
    ) -> Result<collaboration_central_conformance::RuntimeFaultObservation, durable_steps::StoreError>
    {
        self.objects.runtime_control(request).await
    }
    #[allow(
        clippy::too_many_arguments,
        reason = "Native composition takes runtime capabilities and independent deployment policies explicitly."
    )]
    pub async fn new(
        key_source: &str,
        clock: Arc<dyn Clock>,
        observe: impl Fn(CentralObservation) + Send + Sync + 'static,
        root: &std::path::Path,
        registry: fleet_sync_lease::RegistryConfig,
        delivery: crate::DeliveryConfig,
        holder_answer_timeout: Duration,
        copy_ack_wait: Duration,
    ) -> Result<Self, TokioPlatformError> {
        Self::new_configured(
            key_source,
            clock,
            observe,
            root,
            registry,
            delivery,
            holder_answer_timeout,
            copy_ack_wait,
            #[cfg(feature = "conformance")]
            Vec::new(),
        )
        .await
    }

    #[cfg(feature = "conformance")]
    #[allow(
        clippy::too_many_arguments,
        reason = "Conformance adds only initial per-object arming controls to native composition."
    )]
    pub(crate) async fn new_with_controls(
        key_source: &str,
        clock: Arc<dyn Clock>,
        observe: impl Fn(CentralObservation) + Send + Sync + 'static,
        root: &std::path::Path,
        registry: fleet_sync_lease::RegistryConfig,
        delivery: crate::DeliveryConfig,
        holder_answer_timeout: Duration,
        copy_ack_wait: Duration,
        controls: Vec<collaboration_central_conformance::RuntimeControlRequest>,
    ) -> Result<Self, TokioPlatformError> {
        Self::new_configured(
            key_source,
            clock,
            observe,
            root,
            registry,
            delivery,
            holder_answer_timeout,
            copy_ack_wait,
            controls,
        )
        .await
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "Keep deployment capabilities and test-only arming controls explicit at composition."
    )]
    async fn new_configured(
        key_source: &str,
        clock: Arc<dyn Clock>,
        observe: impl Fn(CentralObservation) + Send + Sync + 'static,
        root: &std::path::Path,
        registry: fleet_sync_lease::RegistryConfig,
        delivery: crate::DeliveryConfig,
        holder_answer_timeout: Duration,
        copy_ack_wait: Duration,
        #[cfg(feature = "conformance")] controls: Vec<
            collaboration_central_conformance::RuntimeControlRequest,
        >,
    ) -> Result<Self, TokioPlatformError> {
        let key_source =
            reqwest::Url::parse(key_source).map_err(|_| TokioPlatformError::InvalidKeySource)?;
        if !matches!(key_source.scheme(), "http" | "https") {
            return Err(TokioPlatformError::InvalidKeySource);
        }
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(TokioPlatformError::HttpClient)?;
        #[cfg(not(feature = "conformance"))]
        let objects = crate::native_objects::NativeObjects::open(
            root,
            clock.clone(),
            registry,
            delivery,
            holder_answer_timeout,
            copy_ack_wait,
        )
        .await?;
        #[cfg(feature = "conformance")]
        let objects = crate::native_objects::NativeObjects::open_with_controls(
            root,
            clock.clone(),
            registry,
            delivery,
            holder_answer_timeout,
            copy_ack_wait,
            controls,
        )
        .await?;
        Ok(Self {
            client,
            key_source,
            clock,
            observe: Arc::new(observe),
            objects,
            ids: runtime_primitives::UuidV7Ids,
        })
    }

    pub async fn shutdown(&self) -> Result<(), durable_steps::StoreError> {
        self.objects.shutdown().await
    }

    pub(crate) fn begin_shutdown(&self) {
        self.objects.stop.cancel();
    }

    /// Actual per-file metadata results from the most recent Health call.
    #[cfg(feature = "conformance")]
    pub async fn conformance_health_sample(
        &self,
        database: fleet_sync_protocol::DatabaseId,
    ) -> Option<Vec<crate::HealthFileSample>> {
        self.objects.health_sample(database).await
    }

    /// Read-only observation of live holder request correlations.
    #[cfg(feature = "conformance")]
    pub async fn conformance_holder_frames(
        &self,
        service: &message_board::ServiceId,
    ) -> Result<Vec<fleet_sync_protocol::FrameId>, durable_steps::StoreError> {
        self.objects.holder_frames(service).await
    }

    /// Read-only observation of attachments created by the real HTTP upgrade.
    #[cfg(feature = "conformance")]
    pub async fn conformance_socket_origins(
        &self,
        service: &message_board::ServiceId,
    ) -> Result<Vec<AdmissionOrigin>, collaboration_central_core::SocketFailure> {
        self.objects.socket_origins(service).await
    }

    #[cfg(feature = "conformance")]
    pub async fn conformance_schema_report(
        &self,
        peer: fleet_sync_protocol::PeerId,
    ) -> Result<Option<fleet_sync_protocol::ReportSchemaSupport>, durable_steps::StoreError> {
        self.objects.schema_report(peer).await
    }

    /// Explicit fixture pruning; no HTTP or production path exposes it.
    #[cfg(feature = "conformance")]
    pub async fn conformance_prune_fleet_feed(
        &self,
    ) -> Result<(u64, i64), durable_steps::StoreError> {
        self.objects.prune_fleet_feed().await
    }
}

impl CentralPlatform for TokioCentralPlatform {
    type Host = crate::TokioStorageHost<CoreHost<crate::TokioObjectIo>>;
    fn clock(&self) -> &dyn Clock {
        self.clock.as_ref()
    }
    fn ids(&self) -> &dyn runtime_primitives::IdSource {
        &self.ids
    }

    async fn transport_object(
        &self,
        call: SerializedObjectCall,
    ) -> Result<Vec<u8>, durable_steps::CallFailure> {
        let (address, body) = call.into_parts();
        self.objects.call(address, body).await
    }

    async fn upgrade_session(
        &self,
        mut request: axum::http::request::Parts,
        binding: fleet_sync_session::ConnectionBinding,
        admission_origin: AdmissionOrigin,
    ) -> Result<axum::http::Response<axum::body::Body>, UpgradeFailure> {
        if self.objects.stop.is_cancelled() {
            return Err(UpgradeFailure::Refused);
        }
        let upgrade = WebSocketUpgrade::from_request_parts(&mut request, &())
            .await
            .map_err(|_| UpgradeFailure::NotAnUpgrade)?;
        let objects = self.objects.clone();
        let token = objects.tasks.token();
        Ok(upgrade
            .on_upgrade(move |socket| async move {
                let _token = token;
                objects
                    .serve_socket(socket, binding, admission_origin)
                    .await;
            })
            .into_response())
    }

    async fn fetch_access_keys(&self) -> Result<RawJwks, KeyFetchFailure> {
        let response = self
            .client
            .get(self.key_source.clone())
            .send()
            .await
            .map_err(|_| KeyFetchFailure::Unreachable)?;
        let status = response.status();
        let response = response
            .error_for_status()
            .map_err(|_| KeyFetchFailure::BadResponse {
                status: status.as_u16(),
            })?;
        let cache_control = response
            .headers()
            .get(reqwest::header::CACHE_CONTROL)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let body = response
            .bytes()
            .await
            .map_err(|_| KeyFetchFailure::Unreachable)?
            .to_vec();
        Ok(RawJwks {
            body,
            cache_control,
        })
    }

    fn verify_rs256(
        &self,
        key: &AccessJwk,
        signed: &[u8],
        signature: &[u8],
    ) -> impl Future<Output = Result<SignatureValidity, VerifyFailure>> + Send {
        ready(crate::access_crypto::verify_rs256_signature(
            key, signed, signature,
        ))
    }

    fn observe(&self, observation: CentralObservation) {
        (self.observe)(observation);
    }
}
