#![allow(clippy::indexing_slicing)] // Test-only JSON fixture construction; behavior assertions remain strict.
//! Real Control proofs for off-board DMs and retained conversation views.
use super::*;
use chrono::{DateTime, Duration, Utc};
use collaboration_contracts::MachineIdentity;
use collaboration_protocol::PushRecordNotice;
use collaboration_runtime::AutomationRetentionWorker;
use collaboration_service::BoardAvailability;
use collaboration_service::SubscriptionDeliveryService;
use collaboration_service::SubscriptionDeliveryServiceProps;
use collaboration_service::SystemSubscriptionClock;
use collaboration_service::{ProjectAdmission, ProjectAdmissionProps};
use sqlx::{ConnectOptions, Connection};
use std::path::PathBuf;

struct BoardControlFixture {
    _fleet_runtime: board_central_test_support::FleetTestRuntime,
    pushes: Arc<interaction_storage::InteractionStore>,
    board_path: PathBuf,
    baseline: message_board::FeedCursor,
    retention: AutomationRetentionWorker,
    control: ControlHarness,
    _directory: tempfile::TempDir,
}

impl BoardControlFixture {
    async fn start() -> Self {
        let directory = tempfile::tempdir().expect("isolated Control databases");
        let registry = board_fixture::new_registry(directory.path()).expect("fixture enrollment");
        let origin = message_board::ActorOrigin {
            installation: message_board::ServiceId::try_from(SERVICE_ID.to_owned())
                .expect("fixture service"),
            machine: message_board::MachineLabel::try_from("off-board proof".to_owned())
                .expect("fixture machine"),
        };
        let (fleet, fleet_runtime) = board_central_test_support::fleet_client(
            directory.path(),
            Arc::new(board_fixture::PendingRegistration),
            &origin,
        )
        .await
        .expect("real central fixture");
        let fleet = Arc::new(fleet);
        let admission = Arc::new(
            ProjectAdmission::new(ProjectAdmissionProps {
                registry: registry.clone(),
                fleet: fleet.clone(),
                identity: MachineIdentity::new(
                    UuidIdentity::try_from(SERVICE_ID.to_owned()).expect("UUID"),
                    Some("off-board proof"),
                )
                .expect("identity"),
            })
            .expect("admission"),
        );
        let automation = Arc::new(TokioMutex::new(
            AutomationStore::open(&directory.path().join("automation.sqlite"))
                .await
                .expect("real automation store"),
        ));
        let pushes = Arc::new(
            interaction_storage::InteractionStore::open(
                directory.path().join("interaction.sqlite"),
            )
            .await
            .expect("native push store"),
        );
        let delivery = Arc::new(RecordingDelivery::new(Arc::clone(&pushes)));
        let presence = Arc::new(RunningPresence);
        let machine = MachineIdentity::new(
            UuidIdentity::try_from(SERVICE_ID.to_owned()).expect("service UUID"),
            Some("off-board-proof"),
        )
        .expect("machine identity");
        let owner = SubscriptionDeliveryService::new(SubscriptionDeliveryServiceProps {
            board_availability: BoardAvailability::Available(Arc::new(TokioMutex::new(
                admission.subscription_window_store(pushes.clone()),
            ))),
            push_store: pushes.clone(),
            delivery: delivery.clone(),
            presence: presence.clone(),
            machine_identity: machine.clone(),
            clock: Arc::new(SystemSubscriptionClock),
        });
        owner.start().await.expect("real reader owner starts");
        let identity = ServiceIdentity::new(SERVICE_ID, SERVICE_ID)
            .expect("service identity")
            .with_project_admission(admission)
            .with_automation_store(automation.clone())
            .with_session_delivery(delivery)
            .with_subscription_delivery_service(owner.clone(), presence.clone());
        let (identity, mail) = native_mail_owner::NativeMailOwner::start(
            identity,
            board_fixture::fixture_peer().expect("fixture installation"),
            collaboration_service::MailPushDeliveryProps {
                interactions: pushes.clone(),
                actors: owner.clone(),
                presence,
                machine,
                clock: Arc::new(SystemSubscriptionClock),
            },
        )
        .expect("compose native push owner");
        let retention = identity
            .automation_retention_worker()
            .expect("real retention worker");
        let mut control = ControlHarness::start(identity).await;
        control.lifetime = PushFixtureLifetime::NativeDelivery {
            actors: owner,
            mail,
            automation,
        };
        let actor = json!({"kind":"human","humanId":"fixture"});
        let header = || json!({"requestId":message_board::RequestId::generate(),"protocolVersion":1,"actor":actor,"actingFor":null});
        let workspace = message_board::WorkspaceId::generate();
        let board = message_board::BoardId::generate();
        let project = message_board::ProjectId::generate();
        let mut create = header();
        create["workspaceId"] = json!(workspace);
        create["name"] = json!("workspace");
        create["description"] = json!("");
        assert!(
            control
                .served
                .call("workspace_create", create)
                .await
                .expect("workspace")
                .get("result")
                .is_some()
        );
        let mut create = header();
        create["workspaceId"] = json!(workspace);
        create["boardId"] = json!(board);
        create["name"] = json!("board");
        create["description"] = json!("");
        create["kind"] = json!({"kind":"area"});
        assert!(
            control
                .served
                .call("board_create", create)
                .await
                .expect("board")
                .get("result")
                .is_some()
        );
        let mut create = header();
        create["boardId"] = json!(board);
        create["projectId"] = json!(project);
        create["title"] = json!("project");
        board_fixture::prepare_registered_creation(
            &fleet,
            &registry,
            &serde_json::from_value(create.clone()).expect("typed creation input"),
            &message_board::ActorOrigin {
                installation: message_board::ServiceId::try_from(SERVICE_ID.to_owned())
                    .expect("service UUID"),
                machine: message_board::MachineLabel::try_from("off-board proof".to_owned())
                    .expect("machine label"),
            },
            &runtime_primitives::SystemClock,
        )
        .await
        .expect("registered project fixture");
        assert!(
            control
                .served
                .call("project_create", create)
                .await
                .expect("project")
                .get("result")
                .is_some()
        );
        let store = registry
            .existing_project(&project)
            .await
            .expect("registry")
            .expect("native project");
        let board_path = store.path().to_owned();
        let read = store.snapshot().await.expect("snapshot");
        let baseline = read.high_water();
        read.finish().await.expect("finish");
        Self {
            _fleet_runtime: fleet_runtime,
            pushes,
            board_path,
            baseline,
            retention,
            control,
            _directory: directory,
        }
    }

    async fn close(self) {
        let Self {
            _fleet_runtime,
            pushes,
            retention,
            control,
            _directory,
            ..
        } = self;
        drop(retention);
        drop(pushes);
        control.close().await;
        _fleet_runtime
            .shutdown()
            .await
            .expect("stop real central fixture");
        drop(_directory);
    }
}

async fn board_content_counts(
    path: &std::path::Path,
    baseline: message_board::FeedCursor,
) -> (i64, i64) {
    let mut observer = sqlx_turso::TursoConnectOptions::new()
        .filename(path)
        .create_if_missing(false)
        .connect()
        .await
        .expect("native observer");
    let counts = sqlx::query_as::<_, (i64, i64)>(
        "SELECT (SELECT count(*) FROM posts),(SELECT count(*) FROM project_feed WHERE position > ?)",
    )
    .bind(i64::try_from(baseline.position.get()).expect("position"))
    .fetch_one(&mut observer)
    .await
    .expect("count stored board content");
    observer.close().await.expect("close board observer");
    counts
}

#[tokio::test]
async fn control_dm_send_and_read_leave_board_messages_and_activity_empty() {
    let mut fixture = BoardControlFixture::start().await;
    assert_eq!(
        board_content_counts(&fixture.board_path, fixture.baseline).await,
        (0, 0)
    );
    let sender = session("claude-local", "off-board-sender");
    let target = session("codex-local", "off-board-recipient");
    let sent = fixture
        .control
        .call(
            "message/send",
            send_params(&target, &sender, "off-board body"),
        )
        .await;
    assert_eq!(
        sent.pointer("/result/receipt/outcome/kind")
            .and_then(Value::as_str),
        Some("peerMessageWritten")
    );
    let push_id = sent
        .pointer("/result/pushId")
        .and_then(Value::as_str)
        .expect("Control send created a push")
        .to_owned();
    assert_eq!(
        board_content_counts(&fixture.board_path, fixture.baseline).await,
        (0, 0)
    );
    let shown = fixture
        .control
        .call("router/show", json!({"caller":target,"reference":push_id}))
        .await;
    assert_eq!(
        shown.pointer("/result/record/body").and_then(Value::as_str),
        Some("off-board body")
    );
    assert_eq!(
        board_content_counts(&fixture.board_path, fixture.baseline).await,
        (0, 0)
    );
    fixture.close().await;
}

async fn stored_dm(
    fixture: &BoardControlFixture,
    origin: &SessionRef,
    target: &SessionRef,
    created_at: DateTime<Utc>,
    body: &str,
) -> PushId {
    let push_id = PushId::try_from(uuid::Uuid::now_v7().to_string()).expect("UUIDv7 push ID");
    fixture
        .pushes
        .insert_push_record(PushRecordDraft {
            push_id: push_id.clone(),
            kind: PushKind::DirectMessage,
            origin: PushOrigin::Session(origin.clone()),
            origin_router_ref: None,
            target: target.clone(),
            reply_to_push_id: None,
            header_facts: PushHeaderFacts::DirectMessage {
                sender_display_name: None,
            },
            body: Some(body.to_owned()),
            activity: None,
            mode: Some(MessageDelivery::Auto),
            guard: None,
            created_at,
        })
        .await
        .expect("persist a typed DM in the native store");
    push_id
}

fn notices(response: Value) -> Vec<PushRecordNotice> {
    serde_json::from_value(
        response
            .pointer("/result/records")
            .expect("real Control returned notice records")
            .clone(),
    )
    .expect("decode published notice types")
}

fn notice_ids(records: &[PushRecordNotice]) -> Vec<PushId> {
    records
        .iter()
        .map(|record| record.push_id.clone())
        .collect()
}

#[tokio::test]
async fn inbox_and_history_use_literal_short_preview_while_show_keeps_full_body() {
    let mut fixture = BoardControlFixture::start().await;
    let sender = session("claude-local", "preview-sender");
    let caller = session("codex-local", "preview-recipient");
    let body = "🪿".repeat(1_340);
    let sent = fixture
        .control
        .call("message/send", send_params(&caller, &sender, &body))
        .await;
    let push_id = PushId::try_from(
        sent.pointer("/result/pushId")
            .and_then(Value::as_str)
            .expect("message send returns push id")
            .to_owned(),
    )
    .expect("valid push id");
    let link = sent
        .pointer("/result/link")
        .and_then(Value::as_str)
        .expect("message send returns full link")
        .to_owned();
    let expected_tail = format!(
        "\"{}…\" (+1240) 📖 Load full message → {link}",
        "🪿".repeat(100)
    );

    let inbox = notices(
        fixture
            .control
            .call("message/inbox", json!({"caller":caller,"limit":50}))
            .await,
    );
    let inbox_notice = inbox
        .iter()
        .find(|record| record.push_id == push_id)
        .expect("inbox retains the push notice");
    assert!(inbox_notice.line.ends_with(&expected_tail));
    assert!(inbox_notice.line.ends_with(&link));

    let history = notices(
        fixture
            .control
            .call(
                "message/history",
                json!({"caller":caller,"with":sender,"limit":50}),
            )
            .await,
    );
    let history_notice = history
        .iter()
        .find(|record| record.push_id == push_id)
        .expect("history retains the same push notice");
    assert_eq!(history_notice.line, inbox_notice.line);
    assert!(history_notice.line.ends_with(&expected_tail));

    let shown = fixture
        .control
        .call("router/show", json!({"caller":caller,"reference":link}))
        .await;
    assert_eq!(shown.pointer("/result/record/body"), Some(&json!(body)));
    assert!(shown.pointer("/result/record/preview").is_none());
    fixture.close().await;
}

#[tokio::test]
async fn control_inbox_and_history_order_retained_dms_and_omit_expired_records() {
    let mut fixture = BoardControlFixture::start().await;
    // Maintenance accepts an injected wall time; no real-time wait drives expiry.
    let now = DateTime::parse_from_rfc3339("2026-09-01T12:00:00Z")
        .expect("fixed retention clock")
        .with_timezone(&Utc);
    let cutoff = now - Duration::days(30);
    let caller = session("codex-local", "history-recipient");
    let correspondent = session("claude-local", "history-correspondent");
    let other = session("cursor-local", "unrelated-correspondent");
    let expired = stored_dm(
        &fixture,
        &correspondent,
        &caller,
        cutoff - Duration::milliseconds(1),
        "expired body",
    )
    .await;
    let boundary = stored_dm(
        &fixture,
        &correspondent,
        &caller,
        cutoff,
        "exact thirty-day boundary",
    )
    .await;
    let older = stored_dm(
        &fixture,
        &correspondent,
        &caller,
        now - Duration::days(2),
        "older unread",
    )
    .await;
    let read = stored_dm(
        &fixture,
        &correspondent,
        &caller,
        now - Duration::days(1),
        "read conversation message",
    )
    .await;
    let outbound = stored_dm(
        &fixture,
        &caller,
        &correspondent,
        now - Duration::hours(3),
        "outgoing conversation message",
    )
    .await;
    let newest = stored_dm(
        &fixture,
        &correspondent,
        &caller,
        now - Duration::hours(2),
        "newest incoming",
    )
    .await;
    let unrelated = stored_dm(
        &fixture,
        &other,
        &caller,
        now - Duration::hours(1),
        "other correspondent",
    )
    .await;
    let shown = fixture
        .control
        .call("router/show", json!({"caller":caller,"reference":read}))
        .await;
    assert!(
        shown
            .pointer("/result/record/readAt")
            .is_some_and(|value| !value.is_null())
    );
    assert_eq!(
        fixture
            .retention
            .prune_batch(now.timestamp_millis())
            .await
            .expect("real retained-content maintenance"),
        1
    );
    assert!(
        fixture
            .pushes
            .get_push_record(&expired)
            .await
            .expect("inspect expired ID")
            .is_none()
    );
    let inbox = notices(
        fixture
            .control
            .call("message/inbox", json!({"caller":caller,"limit":50}))
            .await,
    );
    assert_eq!(
        notice_ids(&inbox),
        vec![unrelated, newest.clone(), older.clone(), boundary.clone()]
    );
    assert!(inbox.iter().all(|record| record.read_at.is_none()
        && record.target == caller
        && record.created_at >= cutoff));
    let history = notices(
        fixture
            .control
            .call(
                "message/history",
                json!({"caller":caller,"with":correspondent,"limit":50}),
            )
            .await,
    );
    assert_eq!(
        notice_ids(&history),
        vec![newest, outbound, read, older, boundary]
    );
    assert!(history.iter().all(|record| record.created_at >= cutoff));
    assert!(history.iter().any(|record| record.target == correspondent));
    assert!(history.iter().any(|record| record.read_at.is_some()));
    assert_eq!(
        board_content_counts(&fixture.board_path, fixture.baseline).await,
        (0, 0)
    );
    fixture.close().await;
}
