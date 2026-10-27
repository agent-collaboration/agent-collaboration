//! A later native local-store failure cannot strand an earlier known-unsent selection.
#[path = "support/board_fixture.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
pub mod board_fixture;
use board_fixture::{BoardFixture, TestError, accepted};
use message_board::*;
use serde_json::json;
use std::sync::Arc;
#[path = "../../collaboration-runtime/tests/support/native_worker_faults.rs"]
mod native_worker_faults;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test-owned request objects and literal assertions."
)]
async fn failing_second_membership_save_rolls_back_the_first_and_retries_without_restart()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let reader: Identity = serde_json::from_value(fixture.actor.clone())?;
    let path = fixture._directory.path().join("selection.sqlite");
    let interactions = Arc::new(interaction_storage::InteractionStore::open(&path).await?);
    let mut windows = fixture
        ._admission
        .subscription_window_store(interactions.clone());
    let now = chrono::Utc::now();
    let mut projects = Vec::new();
    for _ in 0..2 {
        let (project, _) = fixture.project().await?;
        let mut request = fixture.header();
        request["scope"] = json!({"kind":"project","projectId":project});
        request["policy"] = json!({"mode":"poll","whenIdle":null,"quietSeconds":0,"capSeconds":0,"lifetimeSeconds":3600});
        accepted(&fixture.api.call("thread_subscribe", request).await?);
        projects.push(project);
    }
    projects.sort();
    windows
        .rescan_missing_subscription_windows(&reader, now)
        .await?;
    let mut roots = Vec::new();
    let mut keys = Vec::new();
    for project in &projects {
        let post = PostId::generate();
        let mut request = fixture.header();
        request["actor"] = json!({"kind":"human","humanId":"other"});
        request["projectId"] = json!(project);
        request["postId"] = json!(post);
        request["about"] = json!(fixture.discussion_subject(project).await?);
        request["text"] = json!("pending selection work");
        request["references"] = json!([]);
        request["role"] = json!(null);
        request["watch"] = json!(false);
        request["addressees"] = json!([]);
        accepted(&fixture.api.call("thread_create", request).await?);
        roots.push(ThreadId::from(post));
        let store = fixture
            ._registry
            .existing_project(project)
            .await?
            .ok_or("project")?;
        let mut read = store.snapshot().await?;
        let membership = read
            .subscription_memberships(Some(&reader))
            .await?
            .into_iter()
            .next()
            .ok_or("membership")??;
        keys.push((
            project.clone(),
            membership.envelope.id.clone(),
            membership.activated_at(),
        ));
        read.finish().await?;
    }
    let now = chrono::Utc::now();
    windows
        .rescan_missing_subscription_windows(&reader, now)
        .await?;
    assert_eq!(windows.due_subscription_roots(&reader, now).await?.len(), 2);
    native_worker_faults::execute(interactions.clone(), "CREATE TRIGGER fail_second_selection BEFORE INSERT ON subscription_delivery_progress WHEN NEW.project_id = (SELECT MAX(project_id) FROM subscription_delivery_progress) BEGIN SELECT RAISE(ABORT, 'second-selection-fault'); END", None).await?;
    assert!(
        windows
            .select_subscription_notice(&reader, &roots, now, usize::MAX)
            .await
            .is_err()
    );
    for (project, membership, activated) in &keys {
        let progress = interactions
            .subscription_progress(project, membership, *activated)
            .await?
            .ok_or("pending progress")?;
        assert!(
            progress.roots().iter().all(|root| root
                .window
                .as_ref()
                .is_none_or(|window| window.selected.is_none())),
            "failed batch stranded a selected membership"
        );
    }
    assert_eq!(windows.due_subscription_roots(&reader, now).await?.len(), 2);
    native_worker_faults::execute(
        interactions.clone(),
        "DROP TRIGGER fail_second_selection",
        None,
    )
    .await?;
    let (batch, settlement) = windows
        .select_subscription_notice(&reader, &roots, now, usize::MAX)
        .await?;
    assert_eq!(batch.roots.len(), 2);
    assert_eq!(settlement.roots.len(), 2);
    windows
        .settle_subscription_batch(&reader, &settlement, SubscriptionDeliveryOutcome::Accepted)
        .await?;
    assert!(
        windows
            .due_subscription_roots(&reader, now)
            .await?
            .is_empty()
    );
    fixture.stop().await
}
