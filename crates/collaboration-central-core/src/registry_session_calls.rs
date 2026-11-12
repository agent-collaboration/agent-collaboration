//! Session publication and Welcome reads use the registry's real shared rules.
use crate::FleetKind;

use crate::{
    CallerKind, ObjectHost, ObjectRuntime, PlatformIo,
    caller_context::CallAuthority,
    object_call_envelope::ObjectCallEnvelope,
    object_runtime::ObjectWork,
    registry_peer_lookup::service_for_peer,
    step_driver::{StepCallback, StepDriver, StepResult},
};
use durable_steps::{
    AtomicStore, BoxFuture, CallFailure, CallTarget, NotSentReason, StoreError,
    storage::TransactionScope,
};
use fleet_sync_protocol::ReportSchemaSupport;
use fleet_sync_session::SchemaSupportRegistry;
use message_board::ServiceId;
use runtime_primitives::{Clock, SyncTime};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::convert::Infallible;

enum SessionRegistryReply {
    Report(fleet_sync_protocol::SchemaSupportRecorded),
    Refused,
}

impl<Host: ObjectHost, Io: PlatformIo> ObjectRuntime<FleetKind, Host, Io>
where
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'scalar> i64: sqlx_core::types::Type<Host::Database>
        + sqlx_core::encode::Encode<'scalar, Host::Database>
        + sqlx_core::decode::Decode<'scalar, Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    pub(crate) async fn session_call(
        &self,
        invocation: ObjectCallEnvelope,
    ) -> Result<Vec<u8>, CallFailure> {
        if invocation.source_face != CallerKind::Session
            || !matches!(invocation.authority, CallAuthority::Machine { .. })
        {
            return Err(refused());
        }
        let CallAuthority::Machine {
            installation: service,
        } = invocation.authority
        else {
            return Err(refused());
        };
        if invocation.kind != SchemaSupportRegistry::KIND {
            return Err(refused());
        }
        let _: SchemaSupportRegistry =
            serde_json::from_value(invocation.target).map_err(|_| refused())?;
        let report: ReportSchemaSupport =
            serde_json::from_value(invocation.request).map_err(|_| refused())?;
        let ObjectWork::Fleet {
            host, shared_host, ..
        } = &self.work;
        let (reply, _) = StepDriver::new(host.clone())
            .run(SessionRegistryStep {
                service,
                report,
                now: shared_host.now(),
            })
            .await
            .map_err(|_| CallFailure::AnswerLost)?;
        match reply {
            SessionRegistryReply::Report(recorded) => serde_json::to_vec(&recorded),
            SessionRegistryReply::Refused => return Err(refused()),
        }
        .map_err(|_| CallFailure::AnswerLost)
    }
}

struct SessionRegistryStep {
    service: ServiceId,
    report: ReportSchemaSupport,
    now: SyncTime,
}
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for SessionRegistryStep
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = SessionRegistryReply;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            let peer = self.report.peer;
            if service_for_peer(scope, peer).await?.as_ref() != Some(&self.service) {
                return Ok((SessionRegistryReply::Refused, Vec::new()));
            }
            let report = self.report;
            let previous = fleet_sync_lease::accepted_schema_support(scope, peer)
                .await
                .map_err(StoreError::backend)?;
            let newly_admitted = previous
                .as_ref()
                .is_none_or(|previous| previous.attach_seq < report.attach_seq);
            let answer = fleet_sync_lease::report_schema_support(scope, &report, self.now)
                .await
                .map_err(StoreError::backend)?;
            if newly_admitted {
                let at = chrono::DateTime::from_timestamp_millis(self.now.unix_millis())
                    .ok_or_else(|| StoreError::Corrupt {
                        field: "machine_enrollments.last_seen".into(),
                        message: "Registry clock is outside the stored time range".into(),
                    })?;
                crate::generated_enrollment::record_machine_presence(
                    scope,
                    at.to_rfc3339(),
                    peer.to_string(),
                )
                .await
                .map_err(StoreError::backend)?;
            }
            let reply = SessionRegistryReply::Report(answer);
            Ok((reply, Vec::new()))
        })
    }
}
fn refused() -> CallFailure {
    CallFailure::NotSent {
        reason: NotSentReason::Unreachable { last_seen: None },
    }
}
