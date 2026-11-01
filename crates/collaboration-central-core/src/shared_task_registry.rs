//! Register the library's registry workflows once at the core composition point.

use crate::core_host::{CoreHost, PlatformIo};
use durable_steps::{AtomicStore, Engine, StoreError, storage::TransactionScope};
use fleet_sync_lease::{
    HolderChangeCancelTask, HolderChangeTask, ImportTask, PreviewTakeoverTask, RegistrationTask,
    RegistryConfig, RetirementTask, SettleReservationTask,
};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::sync::Arc;

pub(crate) fn registry_engine<Store, Driver, Io>(
    store: Arc<Store>,
    host: Arc<CoreHost<Io>>,
    config: RegistryConfig,
) -> Result<Engine<Store, CoreHost<Io>>, StoreError>
where
    Store: AtomicStore,
    Driver: Database,
    Io: PlatformIo,
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    let mut engine = Engine::new(store, host.clone());
    engine.register(crate::machine_enrollment::RevokeMachineTask::<Store>(
        std::marker::PhantomData,
    ))?;
    engine.register(
        crate::machine_enrollment::ReadmissionTask::<Store, Driver> {
            marker: std::marker::PhantomData,
            #[cfg(feature = "conformance")]
            probe: host.revocation_probe.clone(),
        },
    )?;
    #[cfg(feature = "conformance")]
    engine.register(crate::conformance_holder::HolderProbeTask::<Store>(
        std::marker::PhantomData,
    ))?;
    #[cfg(feature = "conformance")]
    engine.register(crate::conformance_copy::CopyProbeTask::<Store>(
        std::marker::PhantomData,
    ))?;
    engine.register(RegistrationTask::<Store, Driver>::new(config))?;
    engine.register(ImportTask::<Store, Driver>::new(config))?;
    engine.register(RetirementTask::<Store, Driver>::new(config))?;
    engine.register(PreviewTakeoverTask::<Store, Driver>::new(config))?;
    engine.register(HolderChangeTask::<Store, Driver>::new(config, host.clone()))?;
    engine.register(HolderChangeCancelTask::<Store, Driver>::new(config, host))?;
    engine.register(SettleReservationTask::<Store>::new())?;
    engine.register(crate::machine_notice_outbox::RegistryNoticeTask::<Store>(
        std::marker::PhantomData,
    ))?;
    Ok(engine)
}

/// Fleet registers one central notice workflow; inbound mutations only admit it.
pub(crate) fn fleet_engine<Store: AtomicStore, Driver: Database, Io: PlatformIo>(
    store: Arc<Store>,
    host: Arc<CoreHost<Io>>,
) -> Result<Engine<Store, CoreHost<Io>>, StoreError>
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    let mut engine = Engine::new(store, host);
    engine.register(crate::machine_notice_outbox::FleetNoticeTask::<Store>(
        std::marker::PhantomData,
    ))?;
    engine.register(crate::automation_batch_notifier::AutomationNoticeTask::<
        Store,
        Driver,
    >(std::marker::PhantomData))?;
    Ok(engine)
}
