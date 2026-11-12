//! One dispatcher drives every task that can inhabit the Fleet journal.
use durable_steps::{
    AtomicStore, BoxFuture, DriveReport, Engine, OperationId, OperationStatus, StoreError,
};
use std::sync::Arc;

// Keep SQL driver bounds at construction; callers only drive and observe the journal.
pub(crate) trait FleetTaskEngine: Send + Sync {
    fn drive_due(&self) -> BoxFuture<'_, Result<DriveReport, StoreError>>;
    fn status<'call>(
        &'call self,
        id: &'call OperationId,
    ) -> BoxFuture<'call, Result<OperationStatus, StoreError>>;
}

struct RegisteredFleetEngine<Store: AtomicStore, Host: durable_steps::Host> {
    engine: Engine<Store, Host>,
}
impl<Store: AtomicStore, Host: durable_steps::Host> FleetTaskEngine
    for RegisteredFleetEngine<Store, Host>
{
    fn drive_due(&self) -> BoxFuture<'_, Result<DriveReport, StoreError>> {
        Box::pin(self.engine.drive_due())
    }
    fn status<'call>(
        &'call self,
        id: &'call OperationId,
    ) -> BoxFuture<'call, Result<OperationStatus, StoreError>> {
        Box::pin(self.engine.status(id))
    }
}

pub(crate) fn own_engine<Store: AtomicStore, Host: durable_steps::Host>(
    engine: Engine<Store, Host>,
) -> Arc<dyn FleetTaskEngine> {
    Arc::new(RegisteredFleetEngine { engine })
}
