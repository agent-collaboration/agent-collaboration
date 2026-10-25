//! Resolve local progress against one project snapshot before scanning or selecting it.
use super::*;
use message_board_storage::{CursorValidity, ProjectSnapshot};

impl SubscriptionWindowStore {
    pub(super) async fn reconcile_progress_lineage(
        &self,
        progress: &mut SubscriptionDeliveryProgressProps,
    ) -> Result<(), WindowStoreError> {
        let project = &progress.project;
        let store = self
            .registry
            .existing_project(project)
            .await
            .map_err(|error| project_registry_failure(project, error))?
            .ok_or_else(|| WindowStoreError::RetryProject {
                project: project.clone(),
                error: BoardError::board_unavailable(),
                diagnostic: "project disappeared during subscription lineage check".into(),
            })?;
        let mut read = store
            .snapshot()
            .await
            .map_err(|error| project_storage_failure(project, error))?;
        let reconciled = reconcile_snapshot_progress(&mut read, progress).await;
        read.finish()
            .await
            .map_err(|error| project_storage_failure(&progress.project, error))?;
        reconciled
    }
}

pub(super) async fn reconcile_snapshot_progress(
    read: &mut ProjectSnapshot,
    progress: &mut SubscriptionDeliveryProgressProps,
) -> Result<(), WindowStoreError> {
    let facts = read
        .cursor_validities(&progress.cursors())
        .await
        .map_err(|error| project_storage_failure(&progress.project, error))?;
    let validity = |cursor: FeedCursor| {
        facts
            .get(&(cursor.epoch, cursor.position))
            .copied()
            .ok_or_else(|| WindowStoreError::from(invalid_window()))
    };
    let resolve = |cursor| match validity(cursor)? {
        CursorValidity::Valid => Ok(cursor),
        CursorValidity::Superseded { resume_from } => Ok(resume_from),
        CursorValidity::Invalid => Err(WindowStoreError::from(invalid_window())),
    };
    // Activation is the membership's immutable storage key, not a movable scan anchor.
    if validity(progress.activated_at)? != CursorValidity::Valid {
        return Err(invalid_window().into());
    }
    // Reject every invalid cursor, even when another discarded bound would clear its window.
    if facts.values().any(|fact| *fact == CursorValidity::Invalid) {
        return Err(invalid_window().into());
    }
    progress.scan_through = resolve(progress.scan_through)?;
    for root in &mut progress.roots {
        root.delivered_through = resolve(root.delivered_through)?;
        if let Some(window) = &root.window {
            let mut bounds = vec![window.from, window.through];
            bounds.extend(window.selected.as_ref().map(|selected| selected.through));
            if bounds
                .into_iter()
                .map(validity)
                .collect::<Result<Vec<_>, _>>()?
                .iter()
                .any(|fact| matches!(fact, CursorValidity::Superseded { .. }))
            {
                // A stale count/selection cannot be clamped: rescan the owed current-line range.
                root.window = None;
            }
        }
    }
    Ok(())
}
