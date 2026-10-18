use collaboration_service::ProjectStoreRegistry;
use message_board::ProjectId;
use std::sync::Arc;

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove the test contract; Result propagates fixture errors.
async fn orphaned_projects_reopen_for_ledger_reads_and_cannot_be_opened_for_writing()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let directory = tempfile::tempdir()?;
    let project = ProjectId::generate();
    let registry = ProjectStoreRegistry::new(directory.path());
    let store = registry.open_project(&project).await?;
    let write = store.begin_admission().await?;
    let commit = write.commit_retaining_writer().await?;
    registry.orphan_project(&project, commit).await?;
    assert!(registry.open_project(&project).await.is_err());
    assert!(registry.existing_project(&project).await?.is_none());
    assert!(registry.rejected_project(&project).await?.is_some());
    let reopened = ProjectStoreRegistry::new(directory.path());
    assert!(reopened.rejected_project(&project).await?.is_some());
    assert!(reopened.open_project(&project).await.is_err());
    Ok(())
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove the test contract; Result propagates fixture errors.
async fn registry_opens_one_native_handle_per_project_and_isolates_writer_locks()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let directory = tempfile::tempdir()?;
    let registry = Arc::new(ProjectStoreRegistry::new(directory.path()));
    let first_id = ProjectId::generate();
    let second_id = ProjectId::generate();
    let (first, again) = tokio::try_join!(
        registry.open_project(&first_id),
        registry.open_project(&first_id)
    )?;
    assert!(Arc::ptr_eq(&first, &again));
    let second = registry.open_project(&second_id).await?;
    assert_ne!(first.path(), second.path());
    assert_eq!(
        first.path(),
        directory
            .path()
            .join("projects")
            .join(format!("{}.sqlite", first_id.as_str()))
    );
    let held = first.begin_admission().await?;
    let independent =
        tokio::time::timeout(std::time::Duration::from_secs(1), second.begin_admission()).await??;
    independent.rollback().await?;
    held.rollback().await?;
    Ok(())
}
