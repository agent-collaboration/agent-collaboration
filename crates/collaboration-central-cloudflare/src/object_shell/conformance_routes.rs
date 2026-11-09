//! Private conformance controls on the actual object and platform capabilities.
use super::{
    ObjectPlatform, Request, Response, Result, RuntimeFace, ShellActor, StoreHost, SyncTime, error,
};

impl ShellActor {
    /// The D4 seams Cloudflare offers; the Tokio-only ones answer `unsupported`.
    async fn seam_control(
        &self,
        host: &StoreHost,
        runtime: &RuntimeFace,
        control: collaboration_central_conformance::ObjectSeamControl,
    ) -> Result<Response> {
        use collaboration_central_conformance::{
            ObjectSeamControl, RuntimeControlAnswer, RuntimeControlFailure,
        };
        use collaboration_central_core::ObjectAlarm;
        let answer = match control {
            ObjectSeamControl::ReadAlarm => Ok(RuntimeControlAnswer::Alarm {
                stored: host
                    .alarm()
                    .stored()
                    .await
                    .map_err(error)?
                    .map(SyncTime::unix_millis),
            }),
            ObjectSeamControl::AlarmDelivery { enabled } => {
                self.state
                    .storage()
                    .put(super::event_lines::ALARM_DELIVERY_KEY, enabled)
                    .await?;
                Ok(RuntimeControlAnswer::Done)
            }
            ObjectSeamControl::Seed {
                seed: collaboration_central_conformance::ObjectSeed::StaleAlarm { at },
            } => {
                // Written as is: workerd delivers a past alarm by its own clock.
                host.alarm()
                    .set_exact(Some(SyncTime::from_unix_millis(at)))
                    .await
                    .map_err(error)?;
                Ok(RuntimeControlAnswer::Done)
            }
            ObjectSeamControl::ArmFault { .. }
            | ObjectSeamControl::ReadFault { .. }
            | ObjectSeamControl::DuplicateAlarm
            | ObjectSeamControl::SettleClock { .. }
            | ObjectSeamControl::ReadEvents { .. }
            | ObjectSeamControl::RebuildAtQuiescence { .. }
            | ObjectSeamControl::ReadPending
            | ObjectSeamControl::Barrier { .. }
            | ObjectSeamControl::AwaitBarrier { .. }
            | ObjectSeamControl::ReleaseBarrier { .. } => Err(RuntimeControlFailure::Unsupported),
            ObjectSeamControl::Snapshot => self.snapshot(host, runtime).await,
        };
        match answer {
            Ok(answer) => Response::from_json(&answer),
            Err(failure) => Ok(Response::from_json(&failure)?.with_status(409)),
        }
    }

    /// S8: the durable read, fenced by the socket attachments read before and
    /// after it. An isolate that has not opened its store answers `notOpen`.
    async fn snapshot(
        &self,
        host: &StoreHost,
        runtime: &RuntimeFace,
    ) -> std::result::Result<
        collaboration_central_conformance::RuntimeControlAnswer,
        collaboration_central_conformance::RuntimeControlFailure,
    > {
        use collaboration_central_conformance::{RuntimeControlAnswer, RuntimeControlFailure};
        use collaboration_central_core::conformance_snapshot::{SnapshotRefusal, fenced};
        let refused = |refusal: SnapshotRefusal| {
            worker::console_warn!("S8 snapshot refused: {refusal}");
            match refusal {
                SnapshotRefusal::CorruptRow { table, key, field } => {
                    RuntimeControlFailure::CorruptRow {
                        table: table.to_owned(),
                        key,
                        field: field.to_owned(),
                    }
                }
                SnapshotRefusal::Storage(_) => RuntimeControlFailure::StorageFailed,
                SnapshotRefusal::Unstable => RuntimeControlFailure::SnapshotUnstable,
            }
        };
        if !self.opened.load(std::sync::atomic::Ordering::Acquire) {
            return Err(RuntimeControlFailure::NotOpen);
        }
        let before = host.registration_rows();
        let durable = match runtime {
            RuntimeFace::Fleet(kernel) => kernel.face().conformance_snapshot_durable().await,
            RuntimeFace::Copy(kernel) => kernel.face().conformance_snapshot_durable().await,
        }
        .map_err(refused)?;
        let after = host.registration_rows();
        fenced(before, durable, &after)
            .map(RuntimeControlAnswer::Snapshot)
            .map_err(refused)
    }

    /// Whether `alarmDelivery` allows this alarm; a dropped one emits its line.
    pub(super) async fn alarm_delivery(
        &self,
        address: &collaboration_central_core::ObjectAddress,
    ) -> Result<bool> {
        use collaboration_central_core::event_trace::{AlarmDroppedLine, ConformanceLine};
        let enabled = self
            .state
            .storage()
            .get::<bool>(super::event_lines::ALARM_DELIVERY_KEY)
            .await?
            .unwrap_or(true);
        if !enabled {
            // The object's business time: the persisted conformance clock when set.
            let at = self
                .state
                .storage()
                .get::<collaboration_central_core::RegistryTime>(crate::composed_control::CLOCK_KEY)
                .await?
                .map_or_else(chrono::Utc::now, |now| now.0)
                .timestamp_millis();
            super::event_lines::console_line(ConformanceLine::CentralAlarmDropped(
                &AlarmDroppedLine {
                    object: address.clone(),
                    at,
                },
            ));
        }
        Ok(enabled)
    }

    pub(super) async fn conformance_fetch(
        &self,
        request: &mut Request,
        host: &StoreHost,
        runtime: &RuntimeFace,
    ) -> Result<Option<Response>> {
        #[cfg(feature = "conformance-faults")]
        if request.path() == "/test/request-session-wake" {
            use super::WakeScheduler;
            use durable_steps::{AtomicStore, JournalScope};
            let (connection, at): (
                fleet_protocol::ConnectionId,
                collaboration_central_core::RegistryTime,
            ) = request.json().await?;
            let at = SyncTime::from_unix_millis(at.0.timestamp_millis());
            host.store()
                .transaction(move |scope| Box::pin(async move { scope.ensure_wake(at).await }))
                .await
                .map_err(error)?;
            host.alarm().wake_at(at).await.map_err(error)?;
            return Response::from_json(&host.conformance_session_alarm(connection).await?)
                .map(Some);
        }
        #[cfg(feature = "conformance-faults")]
        if request.path() == "/test/seed-session-alarm" {
            if !matches!(runtime, RuntimeFace::Fleet(_)) {
                return Err(error("alarm fixture requires Fleet"));
            }
            let (now, at): (
                collaboration_central_core::RegistryTime,
                collaboration_central_core::RegistryTime,
            ) = request.json().await?;
            self.state
                .storage()
                .put(crate::composed_control::CLOCK_KEY, &now)
                .await?;
            self.faults.restore_clock(Some(now)).map_err(error)?;
            self.state
                .storage()
                .set_alarm(worker::ScheduledTime::from(at.0))
                .await?;
            return Response::from_json(&()).map(Some);
        }
        if request.path() == "/test/fleet-mail-custody" {
            let RuntimeFace::Fleet(fleet_kernel) = runtime else {
                return Err(error("custody observation requires Fleet"));
            };
            let runtime = fleet_kernel.face();
            let input: collaboration_central_core::FleetMailCustodyRequest = request.json().await?;
            return Response::from_json(
                &runtime
                    .conformance_mail_custody(input.item)
                    .await
                    .map_err(error)?,
            )
            .map(Some);
        }
        if request.path() == "/test/fleet-schema" {
            let RuntimeFace::Fleet(fleet_kernel) = runtime else {
                return Err(error("schema observation requires the Fleet owner"));
            };
            let runtime = fleet_kernel.face();
            return Response::from_json(&runtime.conformance_schema().await.map_err(error)?)
                .map(Some);
        }
        if request.path() == crate::composed_control::OBJECT_PATH {
            let command: collaboration_central_conformance::RuntimeControl = request.json().await?;
            let command = match command.into_seam() {
                Ok(seam) => return self.seam_control(host, runtime, seam).await.map(Some),
                Err(command) => command,
            };
            if matches!(
                command,
                collaboration_central_conformance::RuntimeControl::ProjectUnavailable
                    | collaboration_central_conformance::RuntimeControl::ClearProjectFault
            ) {
                if !matches!(runtime, RuntimeFace::Copy(_)) {
                    return Err(error("project fault requires a project object"));
                }
                self.state
                    .storage()
                    .put(
                        crate::composed_control::PROJECT_FAULT_KEY,
                        matches!(
                            command,
                            collaboration_central_conformance::RuntimeControl::ProjectUnavailable
                        ),
                    )
                    .await?;
            }
            if let collaboration_central_conformance::RuntimeControl::SetTime { now } = &command {
                self.state
                    .storage()
                    .put(crate::composed_control::CLOCK_KEY, now)
                    .await?;
                // Passive: the persisted clock moves; nothing is woken (S3).
                self.faults.restore_clock(Some(*now)).map_err(error)?;
            }
            return crate::composed_control::observation(self.faults.control(command).await)
                .map(Some);
        }
        if request.path() == "/test/forward-state" {
            let RuntimeFace::Fleet(fleet_kernel) = runtime else {
                return Err(error("not a connection runtime"));
            };
            let runtime = fleet_kernel.face();
            return Response::from_json(&runtime.conformance_forward_state().await.map_err(error)?)
                .map(Some);
        }
        if request.path() == "/test/holder-frames" {
            let RuntimeFace::Fleet(fleet_kernel) = runtime else {
                return Err(error("not a connection runtime"));
            };
            let runtime = fleet_kernel.face();
            return Response::from_json(&runtime.conformance_holder_frames().map_err(error)?)
                .map(Some);
        }
        if request.path() == "/test/admission-origins" {
            let mut origins = Vec::new();
            for connection in host.registrations().await.map_err(error)?.keys() {
                origins.push(host.admission_origin(*connection).await.map_err(error)?);
            }
            return Response::from_json(&origins).map(Some);
        }
        if request.path() == "/test/session-connections" {
            if !matches!(runtime, RuntimeFace::Fleet(_)) {
                return Err(error("session observation requires a Connection object"));
            }
            let installation: Option<message_board::ServiceId> = request.json().await?;
            let connections = host
                .conformance_session_connections()
                .map_err(error)?
                .into_iter()
                .filter(|connection| {
                    installation
                        .as_ref()
                        .is_none_or(|installation| connection.belongs_to(installation))
                })
                .collect::<Vec<_>>();
            return Response::from_json(&connections).map(Some);
        }
        if request.path() == "/test/session-faults" {
            if !matches!(runtime, RuntimeFace::Fleet(_)) {
                return Err(error("session faults require a Connection object"));
            }
            let command: collaboration_central_conformance::SessionFaultControl =
                request.json().await?;
            #[cfg(not(feature = "conformance-faults"))]
            if matches!(
                command,
                collaboration_central_conformance::SessionFaultControl::ArmBarrier { .. }
            ) {
                return Err(error("session barrier requires conformance-faults"));
            }
            let connections = host.conformance_session_connections().map_err(error)?;
            match &command {
                collaboration_central_conformance::SessionFaultControl::FailRegistrationPublication { connection }
                | collaboration_central_conformance::SessionFaultControl::ArmBarrier { connection }
                    if !connections.iter().any(|item| item.connection() == *connection && item.is_live()) =>
                {
                    return Err(error("publication fault requires its actual normal socket"));
                }
                collaboration_central_conformance::SessionFaultControl::FailFinalize { .. }
                    if !connections.iter().any(|item| item.is_live()) =>
                {
                    return Err(error("finalize fault requires a normal session socket"));
                }
                _ => {},
            }
            return Response::from_json(&self.faults.session_control(command).map_err(error)?)
                .map(Some);
        }
        #[cfg(feature = "conformance-faults")]
        if request.path() == "/test/session-attachment-fault" {
            let (connection, fault) = request.json().await?;
            return Response::from_json(
                &host
                    .conformance_attachment_fault(connection, fault)
                    .await
                    .map_err(error)?,
            )
            .map(Some);
        }
        if request.path() == "/test/session-attachment-codec" {
            if !matches!(runtime, RuntimeFace::Fleet(_)) {
                return Err(error("codec observation requires a Connection object"));
            }
            return Response::from_json(
                &host
                    .conformance_attachment_codec(request.json().await?)
                    .map_err(error)?,
            )
            .map(Some);
        }
        if request.path() == "/test/session-alarm" {
            let RuntimeFace::Fleet(fleet_kernel) = runtime else {
                return Err(error("alarm observation requires a Connection object"));
            };
            let runtime = fleet_kernel.face();
            let (now, deadline) = runtime.conformance_holder_deadline().map_err(error)?;
            return Response::from_json(
                &host
                    .conformance_session_alarm(request.json().await?)
                    .await?
                    .with_holder_deadline(now, deadline),
            )
            .map(Some);
        }
        if request.path() == "/test/schema-report" {
            let RuntimeFace::Fleet(fleet_kernel) = runtime else {
                return Err(error("not a registry runtime"));
            };
            let runtime = fleet_kernel.face();
            let report = runtime
                .conformance_schema_report(request.json().await?)
                .await
                .map_err(error)?;
            return Response::from_json(&report).map(Some);
        }
        if request.path() == crate::conformance_bootstrap::IMPORTED_UNHELD_PATH {
            let RuntimeFace::Fleet(fleet_kernel) = runtime else {
                return Err(error("import fixture requires Registry"));
            };
            let (database, owner, board_state): (
                fleet_protocol::DatabaseId,
                collaboration_central_core::OwnerIdentity,
                collaboration_central_core::ConformanceBoardState,
            ) = request.json().await?;
            // An owner call event, so what the import owes is sent after it.
            let identity = durable_steps::CallId {
                operation: durable_steps::OperationId::new(durable_steps::Uuid::now_v7())
                    .map_err(error)?,
                step: durable_steps::StepName::new("conformance_setup").map_err(error)?,
            };
            let (_, body) =
                collaboration_central_core::conformance_setup_calls::encode_conformance_setup(
                    owner,
                    collaboration_central_core::conformance_setup_calls::ConformanceImportedUnheld,
                    collaboration_central_core::conformance_setup_calls::ImportedUnheldRequest {
                        database,
                        board_state,
                    },
                    identity,
                )
                .map_err(|failure| error(format!("{failure:?}")))?
                .into_parts();
            let collaboration_central_core::EventReply::CallAnswer(answer) = fleet_kernel
                .on_event(collaboration_central_core::ObjectEvent::Call {
                    body,
                    storage_bytes: None,
                })
                .await
            else {
                return Err(error("imported-unheld setup did not answer"));
            };
            let position: fleet_protocol::ReplicationPosition =
                serde_json::from_slice(&answer).map_err(error)?;
            return Response::from_json(&position).map(Some);
        }
        #[cfg(feature = "conformance-faults")]
        if request.path() == "/test/session-finalize-barrier" {
            crate::conformance_scope_faults::session_barrier(
                host,
                &self.faults,
                request.json().await?,
            )
            .await?;
            return Response::from_json(&()).map(Some);
        }
        #[cfg(feature = "conformance-faults")]
        if request.path() == "/test/session-finalize-journal" {
            let observation =
                crate::conformance_scope_faults::session_journal(host, request.json().await?)
                    .await?;
            return Response::from_json(&observation).map(Some);
        }
        if request.path() == "/test/holder-session-identity" {
            let RuntimeFace::Fleet(fleet_kernel) = runtime else {
                return Err(error("holder identity requires a Connection object"));
            };
            let runtime = fleet_kernel.face();
            let connection: fleet_protocol::ConnectionId = request.json().await?;
            host.conformance_session_alarm(connection).await?;
            runtime
                .conformance_holder_session_identity()
                .map_err(error)?;
            return Response::from_json(&()).map(Some);
        }
        if request.path() == "/test/holder-observation" {
            let RuntimeFace::Fleet(fleet_kernel) = runtime else {
                return Err(error("holder observation requires a Connection object"));
            };
            let runtime = fleet_kernel.face();
            return Response::from_json(&runtime.conformance_holder_observation().map_err(error)?)
                .map(Some);
        }
        Ok(None)
    }
}
