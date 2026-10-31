use codex_router_core::ids::AccountId;
use codex_router_core::provider::Provider;
use codex_router_state::account::{AccountRecord, AccountStatus};
use codex_router_state::quota_snapshot::PersistedQuotaHistoryObservation;
use codex_router_state::sqlite::{AsyncQuotaHistoryRepository, AsyncSqliteStateStore};

use super::{MaintenanceHint, MaintenanceRepository};

#[tokio::test]
async fn maintenance_prunes_quota_history_before_cutoff_and_keeps_boundary_rows() {
    const NOW_UNIX_SECONDS: u64 = 8 * 86_400 + 17;
    let cutoff =
        NOW_UNIX_SECONDS - codex_router_state::sqlite::QUOTA_HISTORY_OBSERVATION_RETENTION_SECONDS;
    let directory = tempfile::tempdir().expect("quota history fixture directory");
    let database_path = directory.path().join("state.sqlite");
    let store = AsyncSqliteStateStore::open(&database_path)
        .await
        .expect("state store should open");
    let account_id =
        AccountId::new("acct_quota_history_retention").expect("retention account should validate");
    store
        .upsert_account(&AccountRecord::new(
            Provider::Openai,
            account_id.clone(),
            account_id.as_str(),
            AccountStatus::Enabled,
        ))
        .await
        .expect("retention account should persist");

    for observed_at in [cutoff - 1, cutoff, cutoff + 1] {
        AsyncQuotaHistoryRepository::append_quota_history_observation(
            &store,
            &PersistedQuotaHistoryObservation::new(
                account_id.clone(),
                account_id.as_str(),
                "responses",
                18_000,
                observed_at,
                50,
            ),
        )
        .await
        .expect("quota history should persist");
    }

    store
        .run_maintenance_hint(MaintenanceHint::CleanupStaleSessionAccountAffinities {
            stale_before_unix_seconds: cutoff,
        })
        .await
        .expect("quota history maintenance should complete");

    assert!(
        AsyncQuotaHistoryRepository::quota_history_observations_for_window(
            &store,
            &account_id,
            "responses",
            18_000,
            0,
            cutoff - 1,
        )
        .await
        .expect("old quota history should load")
        .is_empty()
    );
    assert_eq!(
        AsyncQuotaHistoryRepository::quota_history_observations_for_window(
            &store,
            &account_id,
            "responses",
            18_000,
            cutoff,
            cutoff + 1,
        )
        .await
        .expect("cutoff and newer quota history should load")
        .len(),
        2
    );
    store.close().await.expect("state store should close");
}
