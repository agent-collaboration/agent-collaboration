//! Copy rules retain their transaction certainty at central's store boundary.

use crate::{
    CopyKind, ObjectHost, ObjectRuntime, PlatformIo,
    object_runtime::ObjectWork,
    step_driver::{StepCallback, StepDriver, StepResult},
};
use durable_steps::{AtomicStore, BoxFuture, CallFailure, StoreError, storage::TransactionScope};
use fleet_sync_protocol::{
    AwaitAckedAnswer, CopyStatusAnswer, DatabaseId, ReceiverId, ReplicationPosition,
};
use fleet_sync_receiver::CopyTransactionFailure;
use runtime_primitives::WakeScheduler;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::{convert::Infallible, marker::PhantomData, sync::Arc};

impl<Host: ObjectHost, Io: PlatformIo> ObjectRuntime<CopyKind, Host, Io>
where
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    pub fn copy(
        host: Arc<Host>,
        io: Io,
        receiver: ReceiverId,
        database: DatabaseId,
        copy_ack_wait: std::time::Duration,
    ) -> Self {
        Self {
            work: ObjectWork::Copy {
                host,
                database,
                io: Arc::new(io),
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
            return Err(fleet_sync_receiver::classify_copy_store_error(
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
            host,
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
            let waiter = changes.subscribe(io.new_id(), deadline)?;
            host.wake()
                .wake_at(deadline)
                .await
                .map_err(|_| CallFailure::AnswerLost)?;
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
                    return Err(fleet_sync_receiver::retryable_copy(
                        fleet_sync_protocol::CopyUnavailableReason::Uninitialized,
                    ));
                }
            }
            // read_copy_status has returned from its store boundary. No object,
            // transaction or notifier guard is held while a later event runs.
            waiter.changed().await?;
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

    pub async fn on_alarm(&self) -> Result<(), StoreError> {
        let ObjectWork::Copy {
            host, io, changes, ..
        } = &self.work
        else {
            return Err(StoreError::Aborted {
                reason: "copy runtime has a different face".into(),
            });
        };
        // Expired acknowledgement waits must not depend on follow-up success.
        changes.expire(io.now());
        if let Some(at) = changes.next_deadline() {
            host.wake().wake_at(at).await.map_err(StoreError::backend)?;
        }
        if matches!(
            self.inspection().await?,
            fleet_sync_receiver::CopyInspection::Retired { .. }
        ) {
            return Ok(());
        }
        self.initialize().await?;
        let (next_expiry, _) = StepDriver::new(host.clone())
            .run(crate::copy_staged_parts::ExpireStagedParts { now: io.now() })
            .await?;
        if let Some(at) = next_expiry {
            host.wake().wake_at(at).await.map_err(StoreError::backend)?;
        }
        self.drive_copy_followups().await
    }
    pub async fn initialize(&self) -> Result<usize, StoreError> {
        let ObjectWork::Copy { host, io, .. } = &self.work else {
            return Err(StoreError::Aborted {
                reason: "copy runtime has a different face".into(),
            });
        };
        crate::ObjectBootstrap::new(host.clone(), io.now())
            .initialize(crate::EngineSchema::Copy)
            .await
    }
}

impl<Host: ObjectHost> StepDriver<Host>
where
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
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
            .map_err(fleet_sync_receiver::classify_copy_store_error)?;
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
            let answer = fleet_sync_receiver::copy_status(scope)
                .await
                .map_err(fleet_sync_receiver::receiver_rollback)?;
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
