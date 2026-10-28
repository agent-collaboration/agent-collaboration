//! Central owns the step boundary; the shared store owns its transaction.

use crate::ObjectHost;
use durable_steps::{AtomicStore, BoxFuture, StoreError, storage::TransactionScope};
use sqlx_core::{arguments::IntoArguments, database::Database, encode::Encode, types::Type};
use std::sync::Arc;

/// Effects become available only after the store confirms the step's commit.
/// The constructor belongs to StepDriver, so a rule cannot release effects.
#[derive(Debug)]
pub(crate) struct CommittedEffects<Effect> {
    effects: Vec<Effect>,
}

impl<Effect> CommittedEffects<Effect> {
    pub(crate) fn into_effects(self) -> Vec<Effect> {
        self.effects
    }
}

pub(crate) struct StepDriver<Host: ObjectHost> {
    host: Arc<Host>,
}

impl<Host: ObjectHost> StepDriver<Host>
where
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'value> String: Type<Host::Database> + Encode<'value, Host::Database>,
{
    pub(crate) fn new(host: Arc<Host>) -> Self {
        Self { host }
    }

    pub(crate) async fn run<Callback>(
        &self,
        callback: Callback,
    ) -> Result<(Callback::Output, CommittedEffects<Callback::Effect>), StoreError>
    where
        Callback: StepCallback<Host::Store, Host::Database>,
    {
        let result = self
            .host
            .store()
            .transaction(move |scope| callback.run(scope))
            .await;
        if let Err(error) = &result
            && let Some(failure) = crate::migration_failure::MigrationFailure::from_rollback(error)
        {
            // This separate transaction follows a confirmed rollback marker.
            // Its failure never replaces the original operation's answer.
            let _ = self
                .host
                .store()
                .transaction(move |scope| {
                    Box::pin(async move { crate::migration_failure::record(scope, failure).await })
                })
                .await;
        }
        let (output, effects) = result?;
        Ok((output, CommittedEffects { effects }))
    }
}

pub(crate) type StepResult<Output, Effect> = Result<(Output, Vec<Effect>), StoreError>;

/// Only core-owned, owned-data callbacks can enter a central transaction.
pub(crate) trait StepCallback<Store: AtomicStore, Driver: Database>: Send + 'static {
    type Output: Send + 'static;
    type Effect: Send + 'static;

    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>>;
}
