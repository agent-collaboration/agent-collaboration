//! One dispatcher registers every task that can inhabit the Fleet journal.
use durable_steps::{
    AtomicStore, BoxFuture, DriveReport, Engine, OperationId, OperationStatus, RequestKey,
    StartRefusal, StoreError, storage::TransactionScope,
};
#[cfg(feature = "conformance")]
use fleet_project_lease::ImportTask;
use fleet_project_lease::{PreviewTakeoverTask, RegistrationTask, RetirementTask};
#[cfg(feature = "conformance")]
use fleet_protocol::Import;
use fleet_protocol::{PreviewTakeover, Registration, Retirement};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::{marker::PhantomData, sync::Arc};

pub(crate) enum FleetTaskInput {
    Register(Registration),
    #[cfg(feature = "conformance")]
    Import(Import),
    Retire(Retirement),
    Preview(PreviewTakeover),
}

// Keep SQL driver bounds at construction; callers only admit the four public workflows.
pub(crate) trait FleetTaskEngine: Send + Sync {
    fn start<'call>(
        &'call self,
        key: &'call RequestKey,
        input: FleetTaskInput,
    ) -> BoxFuture<'call, Result<OperationId, StartRefusal>>;
    fn start_registration<'call>(
        &'call self,
        key: &'call RequestKey,
        input: Registration,
    ) -> BoxFuture<'call, Result<OperationId, StartRefusal>> {
        self.start(key, FleetTaskInput::Register(input))
    }
    #[cfg(feature = "conformance")]
    fn start_import<'call>(
        &'call self,
        key: &'call RequestKey,
        input: Import,
    ) -> BoxFuture<'call, Result<OperationId, StartRefusal>> {
        self.start(key, FleetTaskInput::Import(input))
    }
    fn start_retirement<'call>(
        &'call self,
        key: &'call RequestKey,
        input: Retirement,
    ) -> BoxFuture<'call, Result<OperationId, StartRefusal>> {
        self.start(key, FleetTaskInput::Retire(input))
    }
    fn start_preview<'call>(
        &'call self,
        key: &'call RequestKey,
        input: PreviewTakeover,
    ) -> BoxFuture<'call, Result<OperationId, StartRefusal>> {
        self.start(key, FleetTaskInput::Preview(input))
    }
    fn drive_due(&self) -> BoxFuture<'_, Result<DriveReport, StoreError>>;
    fn status<'call>(
        &'call self,
        id: &'call OperationId,
    ) -> BoxFuture<'call, Result<OperationStatus, StoreError>>;
}

struct RegisteredFleetEngine<Store: AtomicStore, Host: durable_steps::Host, Driver, Hook> {
    engine: Engine<Store, Host>,
    driver: PhantomData<fn() -> (Driver, Hook)>,
}
impl<Store: AtomicStore, Host: durable_steps::Host, Driver: Database, Hook> FleetTaskEngine
    for RegisteredFleetEngine<Store, Host, Driver, Hook>
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    for<'transaction> Hook: fleet_project_lease::LeaseChangeHook<Store::Scope<'transaction>>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    fn start<'call>(
        &'call self,
        key: &'call RequestKey,
        input: FleetTaskInput,
    ) -> BoxFuture<'call, Result<OperationId, StartRefusal>> {
        Box::pin(async move {
            match input {
                FleetTaskInput::Register(input) => {
                    self.engine
                        .start::<RegistrationTask<Store, Driver, Hook>>(key, input)
                        .await
                }
                #[cfg(feature = "conformance")]
                FleetTaskInput::Import(input) => {
                    self.engine
                        .start::<ImportTask<Store, Driver, Hook>>(key, input)
                        .await
                }
                FleetTaskInput::Retire(input) => {
                    self.engine
                        .start::<RetirementTask<Store, Driver, Hook>>(key, input)
                        .await
                }
                FleetTaskInput::Preview(input) => {
                    self.engine
                        .start::<PreviewTakeoverTask<Store, Driver>>(key, input)
                        .await
                }
            }
        })
    }
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

pub(crate) fn own_engine<Store: AtomicStore, Host: durable_steps::Host, Driver: Database, Hook>(
    engine: Engine<Store, Host>,
) -> Arc<dyn FleetTaskEngine>
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    for<'transaction> Hook: fleet_project_lease::LeaseChangeHook<Store::Scope<'transaction>>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    Arc::new(RegisteredFleetEngine::<_, _, Driver, Hook> {
        engine,
        driver: PhantomData,
    })
}
