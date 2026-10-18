use collaboration_service::{ProjectStoreRegistry, SubscriptionWindowStore};
use message_board::*;
use std::sync::Arc;
use tokio::sync::Mutex;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove the test contract; Result propagates fixture errors.
async fn local_selection_settles_without_changing_replicated_membership_or_feed() -> TestResult {
    let directory = tempfile::tempdir()?;
    let registry = Arc::new(ProjectStoreRegistry::new(directory.path()));
    let automation = Arc::new(Mutex::new(
        automation_storage::AutomationStore::open(&directory.path().join("automation.sqlite"))
            .await?,
    ));
    let project = ProjectId::generate();
    let store = registry.open_project(&project).await?;
    let owner = Identity::Human {
        human_id: HumanId::try_from("reader".to_owned())?,
    };
    let other = Identity::Human {
        human_id: HumanId::try_from("writer".to_owned())?,
    };
    let now = "2026-10-12T00:00:00Z".parse()?;
    let cursor = FeedCursor {
        epoch: EpochNumber::try_from(1)?,
        position: FeedPosition::ZERO,
    };
    let workspace = WorkspaceId::generate();
    let board = BoardId::generate();
    let envelope = |id, actor| -> TestEnvelope {
        RecordEnvelope {
            id,
            scope: RecordScope {
                workspace: workspace.clone(),
                board: Some(board.clone()),
                project: Some(project.clone()),
            },
            actor: RecordedActor {
                actor: ActorKind::Declared(actor),
                installation: ServiceId::try_from(
                    "00000000-0000-4000-8000-000000000001".to_owned(),
                )
                .unwrap(),
                machine: MachineLabel::try_from("window proof".to_owned()).unwrap(),
            },
            request: None,
            position: RecordPosition::Project(cursor),
            router_time: now,
        }
    };
    let policy = SubscriptionPolicy::new(
        &owner,
        SubscriptionMode::Poll,
        WhenIdle::Hold,
        BatchTiming::new(0, 0)?,
        SubscriptionLifetime::defaults(),
    )?;
    let membership = SubscriptionMembership::new(SubscriptionMembershipProps {
        envelope: envelope(RecordId::generate(), owner.clone()),
        reader: owner.clone(),
        scope: SubscriptionScope::project(project.clone()),
        policy,
        renewed_at: now,
        expires_at: now + chrono::Duration::days(1),
        generation: SubscriptionGeneration::new(1)?,
        activated_at: cursor,
        ended: None,
    })?;
    let membership_id = membership.envelope.id.clone();
    let mut write = store.begin_admission().await?;
    write
        .append_record(Record::SubscriptionMembership(membership))
        .await?;
    write.set_project_watch(&owner, cursor, true).await?;
    write.commit().await?;
    let mut windows = SubscriptionWindowStore::new(registry.clone(), automation.clone());
    windows
        .rescan_missing_subscription_windows(&owner, now)
        .await?;
    let post_id = PostId::generate();
    let thread_id = ThreadId::from(post_id.clone());
    let mut write = store.begin_admission().await?;
    write
        .append_record(Record::Thread(Thread {
            envelope: envelope(thread_id.clone().into(), other.clone()),
            thread_id: thread_id.clone(),
            about: ThreadSubject::Project {
                line: "Fixture discussion".to_owned().try_into()?,
            },
            imported: false,
            state: ThreadState::Open,
            orchestrator: None,
            implementer: None,
        }))
        .await?;
    write
        .append_record(Record::Post(Post {
            envelope: envelope(post_id.clone().into(), other.clone()),
            post_id,
            board_id: board,
            project_id: project.clone(),
            placement: Placement::Thread {
                thread_id: thread_id.clone(),
            },
            actor: other,
            acting_for: None,
            posted_as_role: None,
            text: PostText::try_from("new work".to_owned())?,
            references: PostReferences::try_from(Vec::new())?,
            position: RecordPosition::Project(cursor),
        }))
        .await?;
    write.commit().await?;
    windows
        .rescan_missing_subscription_windows(&owner, now)
        .await?;
    let before = store.snapshot().await?;
    let high = before.high_water();
    before.finish().await?;
    assert_eq!(
        windows.due_subscription_roots(&owner, now).await?,
        vec![thread_id.clone()]
    );
    let (batch, settlement) = windows
        .select_subscription_notice(&owner, &[thread_id], now, usize::MAX)
        .await?;
    assert_eq!(batch.roots.len(), 1);
    assert_eq!(batch.roots[0].post_count, 1);
    assert!(
        windows
            .due_subscription_roots(&owner, now)
            .await?
            .is_empty()
    );
    windows
        .settle_subscription_batch(&owner, &settlement, SubscriptionDeliveryOutcome::Accepted)
        .await?;
    let after = store.snapshot().await?;
    assert_eq!(after.high_water(), high);
    after.finish().await?;
    let progress = automation
        .lock()
        .await
        .subscription_progress(&project, &membership_id, cursor)
        .await?
        .ok_or("local progress missing")?;
    assert!(progress.roots().iter().all(|root| root.window.is_none()));
    assert_eq!(
        progress.last_outcome(),
        Some(&SubscriptionDeliveryOutcome::Accepted)
    );
    Ok(())
}
type TestEnvelope = RecordEnvelope;
