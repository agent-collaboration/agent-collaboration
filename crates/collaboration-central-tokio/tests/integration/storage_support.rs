//! Shared helpers: a held clock, a platform over a temporary root, and row readers.

use collaboration_central_core::storage::{
    ReadOutcome, SqlValue, Statement, StoreCall, StoreReply,
};
use collaboration_central_core::{CentralPlatform, DeliveryConfig};
use collaboration_central_tokio::{StoreMigrations, TokioCentralPlatform};
use message_board::WorkspaceId;
use runtime_primitives::{Clock, SyncTime};
use sqlx::migrate::Migrator;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

/// The one injected clock: it moves only when a case moves it.
pub struct HeldClock(AtomicI64);
impl HeldClock {
    pub fn at(milliseconds: i64) -> Arc<Self> {
        Arc::new(Self(AtomicI64::new(milliseconds)))
    }
}
impl Clock for HeldClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(self.0.load(Ordering::SeqCst))
    }
}

pub fn workspace() -> WorkspaceId {
    WorkspaceId::try_from("019f0000-0000-7000-8000-0000000000aa".to_owned()).unwrap()
}

/// Workspace stores run `workspace_migrator`; central keeps its production (empty) one.
pub async fn open_platform(
    root: &Path,
    clock: Arc<HeldClock>,
    workspace_migrator: Migrator,
) -> TokioCentralPlatform {
    let migrations = StoreMigrations {
        workspace: Arc::new(workspace_migrator),
        ..StoreMigrations::production()
    };
    TokioCentralPlatform::new_with_migrations(
        "http://127.0.0.1:9/cdn-cgi/access/certs",
        clock,
        |_| {},
        root,
        fleet_sync_lease::RegistryConfig {
            lease: fleet_sync_lease::LeasePolicy {
                lease_duration: Duration::from_secs(14400),
                renew_interval: Duration::from_secs(900),
            },
            authoritative_receiver: fleet_sync_protocol::ReceiverId::new(
                uuid::Uuid::parse_str("019f0000-0000-7000-8000-000000000010").unwrap(),
            )
            .unwrap(),
            release_unsent_for: Duration::from_secs(300),
            move_needs_renewal_within: Some(Duration::from_secs(1800)),
        },
        DeliveryConfig {
            relay_peer: fleet_sync_protocol::PeerId::new(
                uuid::Uuid::parse_str("019f0000-0000-7000-8000-000000000011").unwrap(),
            )
            .unwrap(),
        },
        migrations,
    )
    .await
    .unwrap()
}

pub async fn read_store(platform: &TokioCentralPlatform, statement: Statement) -> ReadOutcome {
    match platform
        .call_store(workspace(), StoreCall::Read(statement))
        .await
        .unwrap()
    {
        StoreReply::Read(outcome) => outcome,
        StoreReply::Batch(outcome) => panic!("a read answered a batch: {outcome:?}"),
    }
}

pub fn single_integer(outcome: &ReadOutcome) -> i64 {
    match outcome.rows.as_slice() {
        [row] => match row.as_slice() {
            [SqlValue::Integer(value)] => value.get(),
            other => panic!("expected one integer, got {other:?}"),
        },
        other => panic!("expected one row, got {other:?}"),
    }
}

pub fn texts_in_column(outcome: &ReadOutcome, column: usize) -> Vec<String> {
    outcome
        .rows
        .iter()
        .map(|row| match row.get(column) {
            Some(SqlValue::Text(value)) => value.clone(),
            other => panic!("expected text, got {other:?}"),
        })
        .collect()
}
