use std::env;
use std::path::PathBuf;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::mpsc;

use codex_router_core::affinity::AffinityKeyHash;
use codex_router_core::ids::AccountId;
use codex_router_core::provider::Provider;
use codex_router_core::routes::RouteBand;
use codex_router_state::account::AccountRecord;
use codex_router_state::account::AccountStatus;
use codex_router_state::affinity_owner::AffinitySourceTransport;
use codex_router_state::affinity_owner::PreviousResponseAffinityOwnerLookup;
use codex_router_state::affinity_owner::PreviousResponseAffinityOwnerRecord;
use codex_router_state::sqlite::AsyncAffinityRepository;
use codex_router_state::sqlite::AsyncSqliteStateStore;

use super::MaintenanceActor;
use super::MaintenanceCompletion;
use super::MaintenanceEnqueueResult;
use super::MaintenanceHint;
use super::previous_response_affinity_owner_retention_cutoff;

static TEMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

#[test]
fn previous_response_affinity_owner_retention_cutoff_uses_injected_clock() {
    const INJECTED_NOW_UNIX_SECONDS: u64 = 31 * 86_400;

    assert_eq!(
        previous_response_affinity_owner_retention_cutoff(INJECTED_NOW_UNIX_SECONDS),
        86_400
    );
    assert_eq!(previous_response_affinity_owner_retention_cutoff(10), 0);
}

#[tokio::test]
async fn maintenance_actor_prunes_old_response_affinity_owner_retention_and_keeps_cutoff_row() {
    const INJECTED_NOW_UNIX_SECONDS: u64 = 90 * 86_400;
    let cutoff_unix_seconds =
        previous_response_affinity_owner_retention_cutoff(INJECTED_NOW_UNIX_SECONDS);
    let database_path = test_database_path();
    let store = AsyncSqliteStateStore::open(&database_path)
        .await
        .unwrap_or_else(|error| panic!("test state store should open: {error}"));
    let account_id = AccountId::new("acct_affinity_retention")
        .unwrap_or_else(|error| panic!("test account should validate: {error}"));
    store
        .upsert_account(&AccountRecord::new(
            Provider::Openai,
            account_id.clone(),
            account_id.as_str(),
            AccountStatus::Enabled,
        ))
        .await
        .unwrap_or_else(|error| panic!("retention fixture account should persist: {error}"));
    let old_hash = AffinityKeyHash::new("a".repeat(64))
        .unwrap_or_else(|error| panic!("test affinity hash should validate: {error}"));
    let cutoff_hash = AffinityKeyHash::new("b".repeat(64))
        .unwrap_or_else(|error| panic!("test affinity hash should validate: {error}"));
    let old_owner = PreviousResponseAffinityOwnerRecord::new(
        old_hash.clone(),
        account_id.clone(),
        1,
        RouteBand::Responses,
        AffinitySourceTransport::HttpSse,
        cutoff_unix_seconds - 1,
    );
    let cutoff_owner = PreviousResponseAffinityOwnerRecord::new(
        cutoff_hash.clone(),
        account_id,
        1,
        RouteBand::Responses,
        AffinitySourceTransport::HttpSse,
        cutoff_unix_seconds,
    );
    AsyncAffinityRepository::write_previous_response_owner(&store, &old_owner)
        .await
        .unwrap_or_else(|error| panic!("old owner fixture should persist: {error}"));
    AsyncAffinityRepository::write_previous_response_owner(&store, &cutoff_owner)
        .await
        .unwrap_or_else(|error| panic!("cutoff owner fixture should persist: {error}"));

    let actor = MaintenanceActor::start(store.clone(), 8);
    let (completion_sender, completion_receiver) = mpsc::channel::<MaintenanceCompletion>();
    actor.register_completion_sender(completion_sender);
    assert_eq!(
        actor.try_enqueue(MaintenanceHint::PrunePreviousResponseAffinityOwners {
            created_before_unix_seconds: cutoff_unix_seconds,
        }),
        MaintenanceEnqueueResult::Enqueued
    );
    let completion = tokio::task::spawn_blocking(move || completion_receiver.recv())
        .await
        .unwrap_or_else(|error| panic!("maintenance completion receiver should join: {error}"))
        .unwrap_or_else(|error| {
            panic!("maintenance actor should report its completed hint: {error}")
        });
    assert_eq!(
        completion.maintenance_class(),
        "previous_response_affinity_owner_retention"
    );
    assert_eq!(completion.route_band(), "all");

    assert_eq!(
        AsyncAffinityRepository::load_previous_response_owner(
            &store,
            &old_hash,
            RouteBand::Responses.as_str(),
        )
        .await,
        Ok(PreviousResponseAffinityOwnerLookup::Missing)
    );
    assert_eq!(
        AsyncAffinityRepository::load_previous_response_owner(
            &store,
            &cutoff_hash,
            RouteBand::Responses.as_str(),
        )
        .await,
        Ok(PreviousResponseAffinityOwnerLookup::Found(cutoff_owner))
    );

    actor.shutdown().await;
    store
        .close()
        .await
        .unwrap_or_else(|error| panic!("test state store should close: {error}"));
    std::fs::remove_file(&database_path)
        .unwrap_or_else(|error| panic!("test state database should be removed: {error}"));
}

fn test_database_path() -> PathBuf {
    let process_id = std::process::id();
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    env::temp_dir().join(format!(
        "codex-router-proxy-affinity-retention-{process_id}-{counter}.sqlite"
    ))
}
