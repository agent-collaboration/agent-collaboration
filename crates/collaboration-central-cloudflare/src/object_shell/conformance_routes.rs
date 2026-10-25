//! Private conformance controls on the actual object and platform capabilities.
use super::*;

impl ShellActor {
    pub(super) async fn conformance_fetch(
        &self,
        request: &mut Request,
        host: &StoreHost,
        runtime: &RuntimeFace,
    ) -> Result<Option<Response>> {
        if request.path() == crate::composed_control::OBJECT_PATH {
            let command: collaboration_central_conformance::RuntimeControl = request.json().await?;
            if matches!(
                command,
                collaboration_central_conformance::RuntimeControl::SmallMailboxCapacity
            ) {
                let RuntimeFace::Mailbox(mailbox) = runtime else {
                    return Err(error("small capacity requires a mailbox"));
                };
                mailbox
                    .conformance_small_mailbox_capacity()
                    .map_err(error)?;
            }
            if let collaboration_central_conformance::RuntimeControl::SetTime { now } = &command {
                self.state
                    .storage()
                    .put(crate::composed_control::CLOCK_KEY, now)
                    .await?;
                self.faults.restore_clock(Some(*now)).map_err(error)?;
                host.wake()
                    .wake_at(SyncTime::from_unix_millis(
                        chrono::Utc::now().timestamp_millis(),
                    ))
                    .await
                    .map_err(error)?;
            }
            return crate::composed_control::observation(self.faults.control(command).await)
                .map(Some);
        }
        if request.path() == "/test/forward-state" {
            let RuntimeFace::Session(runtime) = runtime else {
                return Err(error("not a connection runtime"));
            };
            return Response::from_json(&runtime.conformance_forward_state().await.map_err(error)?)
                .map(Some);
        }
        if request.path() == "/test/holder-frames" {
            let RuntimeFace::Session(runtime) = runtime else {
                return Err(error("not a connection runtime"));
            };
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
            if !matches!(runtime, RuntimeFace::Session(_)) {
                return Err(error("session observation requires a Connection object"));
            }
            return Response::from_json(&host.conformance_session_connections().map_err(error)?)
                .map(Some);
        }
        if request.path() == "/test/session-faults" {
            if !matches!(runtime, RuntimeFace::Session(_)) {
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
        if request.path() == "/test/session-attachment-codec" {
            if !matches!(runtime, RuntimeFace::Session(_)) {
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
            if !matches!(runtime, RuntimeFace::Session(_)) {
                return Err(error("alarm observation requires a Connection object"));
            }
            return Response::from_json(
                &host
                    .conformance_session_alarm(request.json().await?)
                    .await?,
            )
            .map(Some);
        }
        if request.path() == "/test/schema-report" {
            let RuntimeFace::Registry(runtime) = runtime else {
                return Err(error("not a registry runtime"));
            };
            let report = runtime
                .conformance_schema_report(request.json().await?)
                .await
                .map_err(error)?;
            return Response::from_json(&report).map(Some);
        }
        if request.path() == crate::conformance_bootstrap::IMPORTED_UNHELD_PATH {
            let RuntimeFace::Registry(runtime) = runtime else {
                return Err(error("import fixture requires Registry"));
            };
            let (database, owner, board_state): (
                fleet_sync_protocol::DatabaseId,
                collaboration_central_core::OwnerIdentity,
                collaboration_central_core::ConformanceBoardState,
            ) = request.json().await?;
            let position = runtime
                .conformance_imported_unheld(database, owner, board_state)
                .await
                .map_err(error)?;
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
            let RuntimeFace::Session(runtime) = runtime else {
                return Err(error("holder identity requires a Connection object"));
            };
            let connection: fleet_sync_protocol::ConnectionId = request.json().await?;
            host.conformance_session_alarm(connection).await?;
            runtime
                .conformance_holder_session_identity()
                .map_err(error)?;
            return Response::from_json(&()).map(Some);
        }
        if request.path() == "/test/holder-observation" {
            let RuntimeFace::Session(runtime) = runtime else {
                return Err(error("holder observation requires a Connection object"));
            };
            return Response::from_json(&runtime.conformance_holder_observation().map_err(error)?)
                .map(Some);
        }
        Ok(None)
    }
}
