//! Fault SQL borrows the broker's native owner rather than opening another engine.
use super::ServiceInteractionBroker;
use durable_steps::{
    AtomicStore, CallFailure, CallId, CallTarget, Host, NotSentReason, StoreError,
    storage::TransactionScope, turso_store::TursoScope,
};
use interaction_storage::InteractionAtomicStore;
use runtime_primitives::{Clock, IdSource, SyncTime, WakeFailure, WakeScheduler};
use std::sync::Arc;

struct HistorySqlHost;
impl Clock for HistorySqlHost {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(0)
    }
}
impl IdSource for HistorySqlHost {
    fn new_id(&self) -> durable_steps::Uuid {
        uuid::Uuid::now_v7()
    }
}
impl WakeScheduler for HistorySqlHost {
    async fn wake_at(&self, _: SyncTime) -> Result<(), WakeFailure> {
        Ok(())
    }
}
impl Host for HistorySqlHost {
    async fn dispatch<TCall: CallTarget>(
        &self,
        _: TCall,
        _: TCall::Request,
        _: CallId,
    ) -> Result<TCall::Answer, CallFailure> {
        Err(CallFailure::NotSent {
            reason: NotSentReason::Overloaded,
        })
    }
}

type HistoryDatabase = <TursoScope<'static, HistorySqlHost> as TransactionScope>::Database;
type HistoryConnection = <HistoryDatabase as sqlx::Database>::Connection;
type HistoryConnectOptions = <HistoryConnection as sqlx::Connection>::Options;

pub(super) struct IndependentNativeHistoryRead {
    connection: HistoryConnection,
}
impl IndependentNativeHistoryRead {
    pub(super) async fn open(path: &std::path::Path) -> Result<Self, sqlx::Error> {
        // A second connection uses the existing native engine, not a second
        // store owner. Opposed read-only engine admission is invalid beside
        // its live writer; this private observer exposes only SELECT.
        let options = HistoryConnectOptions::new()
            .filename(path)
            .create_if_missing(false);
        let connection = <HistoryConnection as sqlx::Connection>::connect_with(&options).await?;
        Ok(Self { connection })
    }

    pub(super) async fn record_json(&mut self, request: &str) -> Result<String, sqlx::Error> {
        sqlx::query_scalar::<HistoryDatabase, String>(
            "SELECT record_json FROM typed_interaction_history WHERE request_id=?",
        )
        .bind(request)
        .fetch_one(&mut self.connection)
        .await
    }

    pub(super) async fn close(self) -> Result<(), sqlx::Error> {
        <HistoryConnection as sqlx::Connection>::close(self.connection).await
    }
}

pub(super) struct NativeHistoryFaults {
    store: InteractionAtomicStore<HistorySqlHost>,
}
impl NativeHistoryFaults {
    pub(super) fn new(broker: &ServiceInteractionBroker) -> Self {
        Self {
            store: InteractionAtomicStore::new(
                broker.interaction_store(),
                Arc::new(HistorySqlHost),
            ),
        }
    }

    pub(super) async fn execute_all(
        &self,
        statements: &'static [&'static str],
    ) -> Result<(), StoreError> {
        self.store
            .transaction(move |scope| {
                Box::pin(async move {
                    for &statement in statements {
                        sqlx::raw_sql(statement).execute(scope.executor()).await?;
                    }
                    Ok(())
                })
            })
            .await
    }

    pub(super) async fn text(&self, statement: &'static str) -> Result<String, StoreError> {
        self.store
            .transaction(move |scope| {
                Box::pin(async move {
                    Ok(sqlx::query_scalar::<HistoryDatabase, String>(statement)
                        .fetch_one(scope.executor())
                        .await?)
                })
            })
            .await
    }

    pub(super) async fn integer(&self, statement: &'static str) -> Result<i64, StoreError> {
        self.store
            .transaction(move |scope| {
                Box::pin(async move {
                    Ok(sqlx::query_scalar::<HistoryDatabase, i64>(statement)
                        .fetch_one(scope.executor())
                        .await?)
                })
            })
            .await
    }
}
