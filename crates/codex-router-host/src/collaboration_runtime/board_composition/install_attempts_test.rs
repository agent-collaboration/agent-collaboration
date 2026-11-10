use super::*;
use std::{collections::BTreeSet, sync::Arc};
type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; fixture errors propagate."
)]
async fn install_attempt_reloads_the_same_choice_and_uses_a_fresh_choice_for_another_epoch()
-> TestResult {
    let root = tempfile::tempdir()?;
    let project = ProjectId::generate();
    let epoch = Epoch::new(2)?;
    let cache_store = message_board_storage::FleetStore::open(
        &root
            .path()
            .join(message_board_storage::FleetStore::FILE_NAME),
    )
    .await?;
    let cache = cache_store.cache();
    let original = recorded_attempt(&cache, &project, epoch).await?;
    assert_eq!(recorded_attempt(&cache, &project, epoch).await?, original);
    assert_ne!(
        recorded_attempt(&cache, &project, Epoch::new(3)?).await?,
        original
    );
    cache_store.close().await;
    let cache_store = message_board_storage::FleetStore::open(
        &root
            .path()
            .join(message_board_storage::FleetStore::FILE_NAME),
    )
    .await?;
    let cache = cache_store.cache();
    assert_eq!(recorded_attempt(&cache, &project, epoch).await?, original);
    assert!(!root.path().join("snapshot-install-attempts").exists());
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; fixture errors propagate."
)]
async fn concurrent_install_attempt_creators_all_observe_the_same_committed_choice() -> TestResult {
    let root = tempfile::tempdir()?;
    let project = ProjectId::generate();
    let epoch = Epoch::new(2)?;
    let cache = Arc::new(
        message_board_storage::FleetStore::open(
            &root
                .path()
                .join(message_board_storage::FleetStore::FILE_NAME),
        )
        .await?
        .cache(),
    );
    let barrier = Arc::new(tokio::sync::Barrier::new(6));
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..6 {
        let cache = cache.clone();
        let project = project.clone();
        let barrier = barrier.clone();
        tasks.spawn(async move {
            barrier.wait().await;
            recorded_attempt(&cache, &project, epoch).await
        });
    }
    let mut choices = BTreeSet::new();
    while let Some(result) = tasks.join_next().await {
        choices.insert(result??);
    }
    assert_eq!(choices.len(), 1);
    assert!(choices.contains(&recorded_attempt(&cache, &project, epoch).await?));
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; fixture errors propagate."
)]
async fn corrupt_legacy_attempt_refuses_startup_without_replacement() -> TestResult {
    for corrupt in [
        serde_json::json!({"epoch":0}),
        serde_json::json!({"epoch":2,"attempt":"550e8400-e29b-41d4-a716-446655440000"}),
    ] {
        let root = tempfile::tempdir()?;
        let project = ProjectId::generate();
        let directory = root.path().join("snapshot-install-attempts");
        tokio::fs::create_dir(&directory).await?;
        let path = directory.join(format!("{}-e2.json", project.as_str()));
        let mut corrupt = corrupt;
        corrupt["project"] = serde_json::to_value(&project)?;
        let bytes = serde_json::to_vec(&corrupt)?;
        tokio::fs::write(&path, &bytes).await?;
        let cache_store = message_board_storage::FleetStore::open(
            &root
                .path()
                .join(message_board_storage::FleetStore::FILE_NAME),
        )
        .await?;
        let cache = cache_store.cache();
        assert!(cache.import_legacy_runtime(root.path()).await.is_err());
        assert_eq!(tokio::fs::read(&path).await?, bytes);
    }
    Ok(())
}
