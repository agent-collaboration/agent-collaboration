//! Copy rules retain their transaction certainty at central's store boundary.
use runtime_primitives::{Clock as _, IdSource as _};

use crate::ObjectIo as _;
use crate::{
    CopyKind, ObjectPlatform, ObjectRuntime,
    object_runtime::ObjectWork,
    step_driver::{StepCallback, StepDriver, StepResult},
};
use durable_steps::{AtomicStore, BoxFuture, CallFailure, StoreError, storage::TransactionScope};
use fleet_project_copy::CopyTransactionFailure;
use fleet_protocol::{
    AwaitAckedAnswer, CopyStatusAnswer, DatabaseId, ReceiverId, ReplicationPosition,
};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::{convert::Infallible, marker::PhantomData, sync::Arc};

impl<Host: ObjectPlatform> ObjectRuntime<CopyKind, Host>
where
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'scalar> i64: sqlx_core::types::Type<Host::Database>
        + sqlx_core::encode::Encode<'scalar, Host::Database>
        + sqlx_core::decode::Decode<'scalar, Host::Database>,
    for<'blob> Vec<u8>: sqlx_core::types::Type<Host::Database>
        + sqlx_core::encode::Encode<'blob, Host::Database>
        + sqlx_core::decode::Decode<'blob, Host::Database>,
    for<'blob> Option<Vec<u8>>:
        sqlx_core::types::Type<Host::Database> + sqlx_core::decode::Decode<'blob, Host::Database>,
    for<'scalar> Option<i64>:
        sqlx_core::types::Type<Host::Database> + sqlx_core::decode::Decode<'scalar, Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    pub fn copy(
        host: Arc<Host>,
        receiver: ReceiverId,
        database: DatabaseId,
        copy_ack_wait: std::time::Duration,
    ) -> Self {
        let io = host.io().clone();
        Self {
            work: ObjectWork::Copy {
                host,
                database,
                io,
                receiver,
                changes: Arc::default(),
                copy_ack_wait,
                #[cfg(feature = "conformance")]
                wait_probe: Default::default(),
            },
            face: PhantomData,
        }
    }

    pub async fn read_copy_status(&self) -> Result<CopyStatusAnswer, CopyTransactionFailure> {
        let ObjectWork::Copy { host, database, .. } = &self.work else {
            return Err(fleet_project_copy::classify_copy_store_error(
                StoreError::Aborted {
                    reason: "copy runtime has a different face".into(),
                },
            ));
        };
        StepDriver::new(host.clone())
            .read_copy_status(*database)
            .await
    }

    pub async fn await_copy_acked(
        &self,
        through: ReplicationPosition,
    ) -> Result<AwaitAckedAnswer, CallFailure> {
        let ObjectWork::Copy {
            io,
            changes,
            copy_ack_wait,
            ..
        } = &self.work
        else {
            return Err(CallFailure::AnswerLost);
        };
        let deadline = io.now().after(*copy_ack_wait);
        #[cfg(feature = "conformance")]
        if let ObjectWork::Copy { wait_probe, .. } = &self.work {
            wait_probe
                .enter_attempt()
                .map_err(|_| CallFailure::AnswerLost)?;
        }
        loop {
            // Subscribe first. A commit after this point either changes the
            // status we read, or leaves this waiter ready before we await it.
            let waiter = changes.subscribe(io.new_id())?;
            let status = self.read_wait_status().await?;
            if io.now() >= deadline {
                return Err(CallFailure::AnswerLost);
            }
            match status {
                CopyStatusAnswer::Status(status) if status.acked >= through => {
                    return Ok(AwaitAckedAnswer::Acked {
                        through: status.acked,
                    });
                }
                CopyStatusAnswer::Status(_) => {}
                CopyStatusAnswer::Retired => return Ok(AwaitAckedAnswer::Retired),
                CopyStatusAnswer::NotInitialized => {
                    return Err(fleet_project_copy::retryable_copy(
                        fleet_protocol::CopyUnavailableReason::Uninitialized,
                    ));
                }
            }
            // read_copy_status has returned from its store boundary. No object,
            // transaction or notifier guard is held while a later event runs.
            // The wait is bounded by an event-scoped deadline, dropped with this
            // call; no alarm and no timer outlives the event (H2).
            let remaining = std::time::Duration::from_millis(
                u64::try_from(
                    deadline
                        .unix_millis()
                        .saturating_sub(io.now().unix_millis()),
                )
                .unwrap_or(0),
            );
            let changed = waiter.changed();
            let expiry = io.event_deadline(remaining);
            futures_util::pin_mut!(changed, expiry);
            match futures_util::future::select(changed, expiry).await {
                futures_util::future::Either::Left((changed, _)) => changed?,
                futures_util::future::Either::Right(((), _)) => {
                    return Err(CallFailure::AnswerLost);
                }
            }
        }
    }

    async fn read_wait_status(&self) -> Result<CopyStatusAnswer, CallFailure> {
        let status = self
            .read_copy_status()
            .await
            .map_err(CopyTransactionFailure::call_failure)?;
        #[cfg(feature = "conformance")]
        if let ObjectWork::Copy { wait_probe, .. } = &self.work {
            wait_probe
                .after_status_read()
                .await
                .map_err(|_| CallFailure::AnswerLost)?;
        }
        Ok(status)
    }

    #[cfg(feature = "conformance")]
    pub(crate) fn copy_wait_control(
        &self,
        control: crate::conformance_copy_wait::CopyWaitControl,
    ) -> Result<crate::conformance_copy_wait::CopyWaitObservation, StoreError> {
        let ObjectWork::Copy {
            wait_probe,
            changes,
            ..
        } = &self.work
        else {
            return Err(StoreError::Aborted {
                reason: "copy runtime has a different face".into(),
            });
        };
        wait_probe.control(control, changes.waiting()?, changes.notifications())
    }

    #[cfg(feature = "conformance")]
    pub(crate) fn record_copy_wait_call(
        &self,
        identity: durable_steps::CallId,
    ) -> Result<(), CallFailure> {
        let ObjectWork::Copy { wait_probe, .. } = &self.work else {
            return Err(CallFailure::AnswerLost);
        };
        wait_probe
            .record_call(identity)
            .map_err(|_| CallFailure::AnswerLost)
    }

    /// The Copy's due source: initialize a live copy, then expire its staged parts.
    pub(crate) async fn expire_copy_staged_parts(&self) -> Result<(), StoreError> {
        let ObjectWork::Copy { host, io, .. } = &self.work else {
            return Err(StoreError::Aborted {
                reason: "copy runtime has a different face".into(),
            });
        };
        if matches!(
            self.inspection().await?,
            fleet_project_copy::CopyInspection::Retired { .. }
        ) {
            return Ok(());
        }
        self.initialize().await?;
        // One unit per staged transaction, keyed by its first part's UUIDv7.
        let now = io.now();
        let staged = host
            .store()
            .transaction(|scope| Box::pin(crate::copy_staged_parts::staged(scope)))
            .await?;
        for (key, subject) in staged {
            Box::pin(crate::due_unit::run_due_unit(
                host.store().as_ref(),
                crate::due_unit::DueItem {
                    subject,
                    kind: crate::work_attempts::AttemptKind::PartExpiry,
                    now,
                    attempt_id: io.new_id(),
                },
                move |scope| Box::pin(crate::copy_staged_parts::expire(scope, key, now)),
            ))
            .await;
        }
        Ok(())
    }
    pub async fn initialize(&self) -> Result<usize, StoreError> {
        let ObjectWork::Copy { host, io, .. } = &self.work else {
            return Err(StoreError::Aborted {
                reason: "copy runtime has a different face".into(),
            });
        };
        crate::ObjectBootstrap::new(host.clone(), io.clone())
            .initialize(crate::EngineSchema::Copy)
            .await
    }
}

impl<Host: ObjectPlatform> StepDriver<Host>
where
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'scalar> i64: sqlx_core::types::Type<Host::Database>
        + sqlx_core::encode::Encode<'scalar, Host::Database>
        + sqlx_core::decode::Decode<'scalar, Host::Database>,
    for<'blob> Vec<u8>: sqlx_core::types::Type<Host::Database>
        + sqlx_core::encode::Encode<'blob, Host::Database>
        + sqlx_core::decode::Decode<'blob, Host::Database>,
    for<'blob> Option<Vec<u8>>:
        sqlx_core::types::Type<Host::Database> + sqlx_core::decode::Decode<'blob, Host::Database>,
    for<'scalar> Option<i64>:
        sqlx_core::types::Type<Host::Database> + sqlx_core::decode::Decode<'scalar, Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    pub(crate) async fn read_copy_status(
        &self,
        database: DatabaseId,
    ) -> Result<CopyStatusAnswer, CopyTransactionFailure> {
        // Only a marker returned by a completed rollback establishes no new
        // effect. Begin/commit/rollback errors are classified after that boundary.
        let (answer, _effects) = self
            .run(ReadCopyStatus { database })
            .await
            .map_err(fleet_project_copy::classify_copy_store_error)?;
        Ok(answer)
    }
}

struct ReadCopyStatus {
    database: DatabaseId,
}

impl<Store, Driver> StepCallback<Store, Driver> for ReadCopyStatus
where
    Store: AtomicStore,
    Driver: Database,
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'blob> Vec<u8>: sqlx_core::types::Type<Driver>
        + sqlx_core::encode::Encode<'blob, Driver>
        + sqlx_core::decode::Decode<'blob, Driver>,
    for<'blob> Option<Vec<u8>>:
        sqlx_core::types::Type<Driver> + sqlx_core::decode::Decode<'blob, Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = CopyStatusAnswer;
    type Effect = Infallible;

    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            let answer = fleet_project_copy::copy_status(scope)
                .await
                .map_err(fleet_project_copy::receiver_rollback)?;
            if let CopyStatusAnswer::Status(status) = &answer
                && status.database != self.database
            {
                return Err(StoreError::Corrupt {
                    field: "copy.database".into(),
                    message: "stored copy differs from its object".into(),
                });
            }
            Ok((answer, Vec::new()))
        })
    }
}
