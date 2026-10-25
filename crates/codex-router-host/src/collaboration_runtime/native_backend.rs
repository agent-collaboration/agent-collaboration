//! Publish native backend readiness and own its lifecycle observer.
use super::*;

impl CollaborationRuntime {
    pub async fn backend_ready(
        &mut self,
        at: ObservationTimestamp,
        schema: Option<BackendSchemaEvidence<'_>>,
    ) -> io::Result<CodexGeneration> {
        let timing = std::time::Instant::now();
        crate::debug_readiness_timing::record("publicReadinessBegin", timing);
        // Revalidate executable identity/publication on every generation; only compilation is cached.
        let (schema, payload_schemas) = match schema {
            Some(evidence) => {
                let digest = self
                    .publish_native_schema(BackendSchemaEvidence {
                        running_executable: evidence.running_executable,
                        export: evidence.export,
                    })
                    .await?;
                crate::debug_readiness_timing::record("publicSchemaVerified", timing);
                let compiled = self.payload_cache.resolve(evidence.export.bundle());
                crate::debug_readiness_timing::record("publicValidatorsResolved", timing);
                (Some(digest), compiled)
            }
            None => (None, None),
        };
        let generation = self.publication.ready(
            at.clone(),
            schema.clone(),
            if self.native_digest.as_ref() == schema.as_ref() {
                payload_schemas.clone()
            } else {
                None
            },
        )?;
        crate::debug_readiness_timing::record("publicGenerationPublished", timing);
        self.current_generation = Some(generation.clone());
        if self
            .record_backend(at, collaboration_protocol::BackendStatus::Ready)
            .await
            .is_err()
        {
            tracing::warn!("backend readiness could not be recorded in lifecycle storage");
        }
        if let Some(schemas) = payload_schemas
            && self.start_native_observer(schemas).is_err()
        {
            tracing::warn!("native observer could not start");
        }
        Ok(generation)
    }
    fn start_native_observer(
        &mut self,
        schemas: std::sync::Arc<codex_native_integration::NativePayloadSchemas>,
    ) -> io::Result<()> {
        let Some(journal) = &self.journal else {
            return Ok(());
        };
        if self.observer_task.is_some() {
            return Err(io::Error::other("previous native observer has not retired"));
        }
        let admission = self.publication.admission_gate().acquire()?;
        let store = std::sync::Arc::clone(journal);
        let scope = collaboration_protocol::ObservationScope {
            endpoint: EndpointRef {
                service_id: self.service_id.clone(),
                endpoint_id: EndpointId::try_from("codex-local".to_owned())
                    .map_err(io::Error::other)?,
            },
            generation: Some(admission.generation().clone()),
            observer_id: new_service_uuid()?,
        };
        self.observer_task = Some(tokio::spawn(async move {
            let retired = admission.retirement();
            let connection = tokio::select! {
                _ = retired.cancelled() => return,
                connection = codex_native_integration::NativeProtocolConnection::connect(admission.backend_path()) => connection,
            };
            match connection {
                Ok(connection) => {
                    match lifecycle_observation::NativeObservationStream::new(
                        lifecycle_observation::NativeObservationInputs {
                            connection,
                            store,
                            scope,
                            schemas,
                        },
                    ) {
                        Ok(observer) => {
                            if observer.run(retired).await.is_err() {
                                tracing::warn!("native lifecycle observation stopped");
                            }
                        }
                        Err(_) => tracing::warn!("native lifecycle observer scope rejected"),
                    }
                }
                Err(_) => {
                    let observed_at = chrono::Utc::now()
                        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
                        .try_into();
                    if let Ok(observed_at) = observed_at {
                        let observation = collaboration_protocol::LifecycleObservation {
                            observed_at,
                            source: collaboration_protocol::ObservationSource::ObserverLifecycle,
                            scope,
                            subject: collaboration_protocol::LifecycleSubject::Backend,
                            change: collaboration_protocol::LifecycleChange::CoverageLost,
                        };
                        let _recorded = store
                            .append(&observation, chrono::Utc::now().timestamp())
                            .await;
                    }
                    tracing::warn!("native lifecycle observation connection unavailable");
                }
            }
        }));
        Ok(())
    }
    async fn publish_native_schema(
        &self,
        evidence: BackendSchemaEvidence<'_>,
    ) -> io::Result<SchemaDigest> {
        if evidence.export.executable() != evidence.running_executable {
            return Err(io::Error::other(
                "schema executable differs from running backend",
            ));
        }
        let observed = codex_native_integration::executable_identity(
            evidence.running_executable.canonical_path(),
        )
        .await
        .map_err(io::Error::other)?;
        if &observed != evidence.running_executable {
            return Err(io::Error::other(
                "schema executable changed before publication",
            ));
        }
        // Publication precedes the directory announcement; readers never receive a missing bundle.
        evidence
            .export
            .bundle()
            .publish(&self.directory)
            .map_err(io::Error::other)?;
        let mut encoded = String::from("sha256:");
        for byte in evidence.export.bundle().digest() {
            use std::fmt::Write as _;
            write!(encoded, "{byte:02x}").map_err(io::Error::other)?;
        }
        SchemaDigest::try_from(encoded).map_err(io::Error::other)
    }
    pub async fn backend_unavailable(
        &mut self,
        at: ObservationTimestamp,
        reason: NonEmptyText,
    ) -> io::Result<()> {
        self.publication.unavailable(at.clone(), reason)?;
        self.drain_native_observer().await;
        if self
            .record_backend(at, collaboration_protocol::BackendStatus::Unavailable)
            .await
            .is_err()
        {
            tracing::warn!("backend loss could not be recorded in lifecycle storage");
        }
        Ok(())
    }
    pub(super) async fn drain_native_observer(&mut self) {
        if let Some(observer) = self.observer_task.take() {
            let _drained = observer.await;
        }
    }
    async fn record_backend(
        &self,
        at: ObservationTimestamp,
        status: collaboration_protocol::BackendStatus,
    ) -> io::Result<()> {
        let Some(journal) = &self.journal else {
            return Ok(());
        };
        let observation = collaboration_protocol::LifecycleObservation {
            observed_at: at,
            source: collaboration_protocol::ObservationSource::HostLifecycle,
            scope: collaboration_protocol::ObservationScope {
                endpoint: EndpointRef {
                    service_id: self.service_id.clone(),
                    endpoint_id: EndpointId::try_from("codex-local".to_owned())
                        .map_err(io::Error::other)?,
                },
                generation: self.current_generation.clone(),
                observer_id: self.service_epoch.clone(),
            },
            subject: collaboration_protocol::LifecycleSubject::Backend,
            change: collaboration_protocol::LifecycleChange::BackendStatus { status },
        };
        journal
            .append(&observation, unix_seconds()?)
            .await
            .map_err(io::Error::other)?;
        Ok(())
    }
}
