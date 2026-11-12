//! Register the enrollment workflows once at the core composition point.

use crate::core_host::{CoreHost, PlatformIo};
use durable_steps::{AtomicStore, Engine, StoreError, storage::TransactionScope};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::sync::Arc;

pub(crate) fn registry_engine<Store, Driver, Io>(
    store: Arc<Store>,
    host: Arc<CoreHost<Io>>,
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
    let mut engine = Engine::new(store, host);
    engine.register(crate::machine_enrollment::RevokeMachineTask::<Store>(
        std::marker::PhantomData,
    ))?;
    engine.register(
        crate::machine_enrollment::ReadmissionTask::<Store, Driver> {
            marker: std::marker::PhantomData,
        },
    )?;
    Ok(engine)
}
