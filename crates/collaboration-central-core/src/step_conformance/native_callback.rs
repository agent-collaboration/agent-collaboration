//! Native robustness callbacks stay inside the existing checked step boundary.

use crate::{
    fleet_decision_step::{FleetDecisionStep, persist_decision},
    step_driver::{StepCallback, StepResult},
};
use durable_steps::{AtomicStore, BoxFuture, storage::TransactionScope};
use fleet_admission::FleetRequestResult;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub(super) struct AfterDecisionWrite {
    pub decision: FleetDecisionStep,
    pub after_write: fn(),
}

impl<Store, Driver> StepCallback<Store, Driver> for AfterDecisionWrite
where
    Store: AtomicStore,
    Driver: Database,
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Decode<'value, Driver> + Encode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = ();
    type Effect = FleetRequestResult;

    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            let answer = persist_decision(scope, self.decision).await?;
            (self.after_write)();
            Ok(((), vec![answer]))
        })
    }
}
