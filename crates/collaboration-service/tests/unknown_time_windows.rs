//! Imported history stays covered; only real live admission supplies window time.
#[path = "support/board_fixture.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
pub mod board_fixture;
use collaboration_service::SubscriptionWindowStore;
use interaction_storage::InteractionStore;
use message_board::*;
use message_board_storage::{ImportRowKeys, ImportRowKind, ProjectWriteTransaction};
use std::sync::Arc;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

struct FixtureImportKeys;
impl ImportRowKeys for FixtureImportKeys {
    fn row_key(&self, _: ImportRowKind, _: &[&str]) -> String {
        // Timing proof, not the offline importer's deterministic key derivation.
        RecordId::generate().as_str().to_owned()
    }
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn unknown_imported_post_is_covered_and_only_the_later_live_post_opens_a_window() -> TestResult
{
    let fixture = board_fixture::BoardFixture::start().await?;
    let (project, workstream) = fixture.project().await?;
    let registry = fixture._registry.clone();
    let source = fixture.record_origin()?;
    let author = Identity::Human {
        human_id: HumanId::try_from("author".to_owned())?,
    };
    let reader = Identity::Human {
        human_id: HumanId::try_from("reader".to_owned())?,
    };
    let initial_time = chrono::Utc::now();
    let initial_cursor = FeedCursor {
        epoch: EpochNumber::INITIAL,
        position: FeedPosition::ZERO,
    };
    let envelope = |id, actor: &Identity, time| RecordEnvelope {
        id,
        scope: RecordScope {
            workspace: fixture.workspace.clone(),
            board: Some(fixture.board.clone()),
            project: Some(project.clone()),
        },
        actor: RecordedActor::declared(actor.clone()),
        request: None,
        position: RecordPlacement::Project(ProjectFeedEntry {
            cursor: initial_cursor,
            origin: source.clone(),
        }),
        time,
    };
    let store = registry.open_project(&project).await?;
    let database = registry.database(&project).await?;
    let imported = PostId::generate();
    let thread = ThreadId::from(imported.clone());
    // Imported thread state and LegacyUnknown cannot be produced by ordinary admission.
    let mut write = store.begin_admission(source.clone()).await?;
    write
        .append_record(Record::Thread(Thread {
            envelope: envelope(
                thread.clone().into(),
                &author,
                RecordTime::Router(initial_time),
            ),
            thread_id: thread.clone(),
            about: ThreadSubject::Workstream(workstream),
            imported: true,
            state: ThreadState::Open,
            orchestrator: None,
            implementer: None,
        }))
        .await?;
    write.commit().await?;
    // Real activation owns the existing thread's watch row, rather than fixture SQL.
    let mut subscribe = fixture.request(
        &project,
        serde_json::json!({
            "kind":"threadSubscribe","scope":{"kind":"project","projectId":project},
            "policy":{"mode":"poll","quietSeconds":0,"capSeconds":0},
        }),
    );
    subscribe["actor"] = serde_json::json!(reader);
    let subscribed = fixture
        ._admission
        .submit(&serde_json::from_value(subscribe)?)
        .await?;
    assert!(matches!(
        subscribed.result,
        RequestResult::Decided(RequestDecision::Accepted { .. })
    ));
    let mut read = store.snapshot().await?;
    let membership = read
        .records(RecordKind::SubscriptionMembership)
        .await?
        .into_iter()
        .find_map(|record| match record {
            Record::SubscriptionMembership(membership) if membership.reader() == &reader => {
                Some(membership)
            }
            _ => None,
        })
        .ok_or("membership")?;
    read.finish().await?;
    let membership_id = membership.envelope.id.clone();
    let activated_at = membership.activated_at();
    let interactions = Arc::new(
        InteractionStore::open(
            &fixture
                ._directory
                .path()
                .join("unknown-window-proof.sqlite"),
        )
        .await?,
    );
    let mut windows = SubscriptionWindowStore::new(registry.clone(), interactions.clone());
    assert_eq!(
        windows
            .rescan_missing_subscription_windows(&reader, chrono::Utc::now())
            .await?,
        0
    );
    let baseline = store.snapshot().await?;
    let before = baseline.high_water();
    baseline.finish().await?;
    let mut import = ProjectWriteTransaction::for_import(
        project.clone(),
        database.begin_write().await?,
        fleet_admission::ImportProvenance {
            installation: source.installation.clone(),
            machine: source.machine.clone(),
        },
        Arc::new(FixtureImportKeys),
    )
    .await?;
    let unknown_cursor = import
        .append_record(Record::Post(Post {
            envelope: envelope(imported.clone().into(), &author, RecordTime::LegacyUnknown),
            post_id: imported,
            board_id: fixture.board.clone(),
            project_id: project.clone(),
            placement: Placement::Project {
                project_id: project.clone(),
            },
            actor: author.clone(),
            acting_for: None,
            text: PostText::try_from("imported window activity".to_owned())?,
            references: PostReferences::try_from(Vec::new())?,
            position: RecordPlacement::Project(ProjectFeedEntry {
                cursor: initial_cursor,
                origin: source.clone(),
            }),
            posted_as_role: None,
        }))
        .await?;
    import.commit().await?;
    let live_post = PostId::generate();
    let mut live_request = fixture.request(
        &project,
        serde_json::json!({
            "kind":"threadPost","postId":live_post,"threadId":thread,
            "text":"live window activity","references":[],"addressees":[],
        }),
    );
    live_request["actor"] = serde_json::json!(author);
    let posted = fixture
        ._admission
        .submit(&serde_json::from_value(live_request)?)
        .await?;
    assert!(matches!(
        posted.result,
        RequestResult::Decided(RequestDecision::Accepted { .. })
    ));
    let mut read = store.snapshot().await?;
    let live_head = read.high_water();
    let Some(Record::Post(live)) = read.record(&live_post.into()).await? else {
        return Err("live post missing".into());
    };
    let live_cursor = live
        .envelope
        .position
        .project_cursor()
        .ok_or("live placement")?;
    let RecordTime::Router(live_time) = live.envelope.time else {
        return Err("live Router time missing".into());
    };
    read.finish().await?;
    // Act: real scanner covers the unknown entry and the live request's complete feed.
    assert_eq!(
        windows
            .rescan_missing_subscription_windows(&reader, chrono::Utc::now())
            .await?,
        1
    );
    let progress = interactions
        .subscription_progress(&project, &membership_id, activated_at)
        .await?
        .ok_or("progress")?;
    assert_eq!(progress.scan_through(), live_head);
    let [root] = progress.roots() else {
        return Err("exactly one root".into());
    };
    assert_eq!(root.thread_id, thread);
    let window = root.window.as_ref().ok_or("live window")?;
    assert_eq!(
        (window.from, window.through, window.post_count),
        (live_cursor, live_cursor, 1)
    );
    assert_eq!(
        (window.opened_at, window.last_arrival_at),
        (live_time, live_time)
    );
    let mut read = store.snapshot().await?;
    let FeedRead::Entries {
        entries,
        covered_through,
        ..
    } = read.feed_after(before, 10, false).await?
    else {
        return Err("history entries".into());
    };
    assert_eq!(covered_through, live_head);
    // Ordinary live admission also writes its ledger; both original posts stay in history.
    assert_eq!(entries.len(), 3);
    assert_eq!(
        entries
            .iter()
            .filter(|entry| entry.record.kind == RecordKind::Post)
            .count(),
        2
    );
    assert!(
        entries
            .iter()
            .any(|entry| entry.position == unknown_cursor.position
                && entry.time == RecordTime::LegacyUnknown)
    );
    assert!(
        entries
            .iter()
            .any(|entry| entry.position == live_cursor.position
                && entry.time == RecordTime::Router(live_time))
    );
    assert!(
        entries
            .iter()
            .any(|entry| entry.record.kind == RecordKind::Request
                && entry.position == live_head.position)
    );
    read.finish().await?;
    drop(windows);
    drop(database);
    drop(store);
    drop(registry);
    fixture.stop().await
}
