//! Conformance-only raw owning rows and completed route callbacks, never a forwarding store.
use crate::FleetKind;
use crate::{
    ObjectHost, ObjectRuntime, PlatformIo,
    object_runtime::ObjectWork,
    step_driver::{StepCallback, StepDriver, StepResult},
};
use durable_steps::{AtomicStore, BoxFuture, StoreError, storage::TransactionScope};
use fleet_sync_protocol::FrameId;
use serde::{Deserialize, Serialize};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::{collections::BTreeSet, convert::Infallible, sync::Mutex};

pub const FORWARD_STATE_PATH: &str = "/admin/v1/conformance/forward-state";
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ForwardStateRequest {
    pub service: message_board::ServiceId,
}

/// Raw typed columns preserve malformed values for the independent observer.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ForwardStoredRecord {
    pub id: Option<String>,
    pub forward_frame_id: Option<String>,
    pub side: Option<String>,
    pub request_frame_id: Option<String>,
    pub origin_machine_id: Option<String>,
    pub origin_session_id: Option<String>,
    pub connection_id: Option<String>,
    pub expected_holder_machine_id: Option<String>,
    pub project_id: Option<String>,
    pub phase: Option<String>,
    pub deadline_at: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ForwardStateObservation {
    pub pending: Vec<ForwardStoredRecord>,
    pub delivered: Vec<ForwardStoredRecord>,
    /// In-memory witnesses only; this list has no persistence or restart guarantee.
    pub completed_routes: Vec<FrameId>,
}

#[derive(Default)]
pub(crate) struct ForwardRouteProbe(Mutex<BTreeSet<FrameId>>);
impl ForwardRouteProbe {
    pub(crate) fn completed(&self, frame: FrameId) -> Result<(), StoreError> {
        self.0
            .lock()
            .map_err(|_| StoreError::backend("forward route probe poisoned"))?
            .insert(frame);
        Ok(())
    }
    fn frames(&self) -> Result<Vec<FrameId>, StoreError> {
        Ok(self
            .0
            .lock()
            .map_err(|_| StoreError::backend("forward route probe poisoned"))?
            .iter()
            .copied()
            .collect())
    }
}

impl<Host: ObjectHost, Io: PlatformIo> ObjectRuntime<FleetKind, Host, Io>
where
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'value> String: Type<Host::Database> + Encode<'value, Host::Database>,
    for<'value> Option<String>: Type<Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    pub async fn conformance_forward_state(&self) -> Result<ForwardStateObservation, StoreError> {
        let ObjectWork::Fleet {
            host, shared_host, ..
        } = &self.work
        else {
            return Err(StoreError::backend(
                "forward observer reached a different face",
            ));
        };
        let ((pending, delivered), _) = StepDriver::new(host.clone()).run(ReadForwardRows).await?;
        Ok(ForwardStateObservation {
            pending,
            delivered,
            completed_routes: shared_host.forward_route_probe.frames()?,
        })
    }
}

struct ReadForwardRows;
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for ReadForwardRows
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = (Vec<ForwardStoredRecord>, Vec<ForwardStoredRecord>);
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            let pending = crate::generated_forwards::read_forward_rows(scope, "origin".into())
                .await
                .map_err(StoreError::backend)?;
            let delivered = crate::generated_forwards::read_forward_rows(scope, "holder".into())
                .await
                .map_err(StoreError::backend)?;
            let raw = |rows: Vec<crate::generated_forwards::StoredForwardRow>| {
                rows.into_iter()
                    .map(|row| ForwardStoredRecord {
                        id: row.id,
                        forward_frame_id: row.forward_frame_id,
                        side: row.side,
                        request_frame_id: row.request_frame_id,
                        origin_machine_id: row.origin_machine_id,
                        origin_session_id: row.origin_session_id,
                        connection_id: row.connection_id,
                        expected_holder_machine_id: row.expected_holder_machine_id,
                        project_id: row.project_id,
                        phase: row.phase,
                        deadline_at: row.deadline_at,
                        created_at: row.created_at,
                        updated_at: row.updated_at,
                    })
                    .collect()
            };
            Ok(((raw(pending), raw(delivered)), Vec::new()))
        })
    }
}
