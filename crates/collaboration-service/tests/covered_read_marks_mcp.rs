//! Identified reads mark coverage without keys, ledgers or activity positions.
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
use board_fixture::{BoardFixture, TestError, accepted};
use message_board::*;
use serde_json::{Value, json};
use sqlx::{ConnectOptions, Connection};

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn readable_future_and_mismatched_schemas_report_mark_refusal_without_hiding_data()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let reader: Identity = serde_json::from_value(fixture.actor.clone())?;
    for reason in ["tooNew", "mismatch"] {
        let project = ProjectId::generate();
        let workstream = WorkstreamId::generate();
        let projects = fixture._directory.path().join("projects");
        std::fs::create_dir_all(&projects)?;
        let path = projects.join(format!("{}.sqlite", project.as_str()));
        let seed = message_board_storage::ProjectStore::open(&path, project.clone()).await?;
        let envelope = RecordEnvelope {
            id: project.clone().into(),
            scope: RecordScope {
                workspace: fixture.workspace.clone(),
                board: Some(fixture.board.clone()),
                project: Some(project.clone()),
            },
            actor: RecordedActor {
                actor: ActorKind::Declared(reader.clone()),
                installation: ServiceId::try_from(
                    "00000000-0000-4000-8000-000000000001".to_owned(),
                )?,
                machine: MachineLabel::try_from("native schema read fixture".to_owned())?,
            },
            request: None,
            position: RecordPosition::Project(FeedCursor {
                epoch: EpochNumber::try_from(1)?,
                position: FeedPosition::ZERO,
            }),
            router_time: chrono::Utc::now(),
        };
        let mut write = seed.begin_admission().await?;
        write
            .append_record(Record::Project(ProjectState {
                envelope: envelope.clone(),
                project_id: project.clone(),
                status: None,
                pinned_definition: None,
                revision: SubjectRevision::INITIAL,
                closed_at: None,
            }))
            .await?;
        write
            .append_record(Record::Workstream(Workstream {
                envelope: RecordEnvelope {
                    id: workstream.clone().into(),
                    ..envelope
                },
                workstream_id: workstream,
                project_id: project.clone(),
                name: ResourceName::try_from("retained lane".to_owned())?,
                goal: "read through a refused mark".to_owned(),
                state: WorkstreamState::Open {},
                revision: SubjectRevision::INITIAL,
            }))
            .await?;
        write.commit().await?;
        seed.close().await;
        let mut connection = sqlx_turso::TursoConnectOptions::new()
            .filename(&path)
            .connect()
            .await?;
        if reason == "tooNew" {
            let supported_version = message_board_storage::PROJECT_MIGRATOR
                .iter()
                .map(|migration| migration.version)
                .max()
                .ok_or("project migrations missing")?;
            let changed = sqlx::query("UPDATE _sqlx_migrations SET version = ? WHERE version = ?")
                .bind(supported_version + 1)
                .bind(supported_version)
                .execute(&mut connection)
                .await?;
            assert_eq!(changed.rows_affected(), 1);
        } else {
            sqlx::query("CREATE TABLE unknown_application_table (id TEXT PRIMARY KEY)")
                .execute(&mut connection)
                .await?;
        }
        connection.close().await?;
        let view = fixture
            .api
            .call("view_work", json!({"projectId":project}))
            .await?;
        assert_eq!(
            view.pointer("/result/workstreams/0/name"),
            Some(&json!("retained lane")),
            "{view}"
        );
        assert_eq!(view.pointer("/result/readMark"), None);
        let content = fixture.api.call("feed_read",json!({"projectId":project,"reader":{"kind":"identified","reader":reader},"after":{"epoch":1,"position":0},"limit":100,"includeRedacted":true})).await?;
        assert_eq!(
            content.pointer("/result/readMark/reason/kind"),
            Some(&json!("schemaNotReady"))
        );
        assert_eq!(
            content.pointer("/result/readMark/reason/refusal/kind"),
            Some(&json!(reason))
        );
        let hook = fixture
            .api
            .call(
                "hook_card",
                json!({"projectId":project,"actor":reader,"subject":{"kind":"project"}}),
            )
            .await?;
        assert_eq!(hook.pointer("/result/kind"), Some(&json!("unavailable")));
        assert_eq!(
            hook.pointer("/result/payload/reason/kind"),
            Some(&json!("schemaNotReady"))
        );
        let store = fixture
            ._registry
            .existing_project(&project)
            .await?
            .ok_or("readonly store missing")?;
        let mut snapshot = store.snapshot().await?;
        assert_eq!(snapshot.high_water().position, FeedPosition::try_from(2)?);
        assert!(snapshot.records(RecordKind::ReadMark).await?.is_empty());
        assert!(snapshot.records(RecordKind::Request).await?.is_empty());
        snapshot.finish().await?;
    }
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn an_unwritable_project_still_returns_the_read_and_never_allocates_a_mark()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, workstream) = fixture.project().await?;
    let task = fixture.task(&project, &workstream).await?;
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("store missing")?;
    let snapshot = store.snapshot().await?;
    let high_water = snapshot.high_water();
    snapshot.finish().await?;
    for (grant, reason) in [
        (
            collaboration_service::GrantResult::NotHolder {
                current: Some(EpochNumber::try_from(2)?),
            },
            "notWriter",
        ),
        (
            collaboration_service::GrantResult::Expired {
                expired_at: chrono::Utc::now(),
            },
            "leaseExpired",
        ),
        (
            collaboration_service::GrantResult::Holder {
                epoch: EpochNumber::try_from(1)?,
                deadline: std::time::Instant::now() - std::time::Duration::from_secs(1),
            },
            "leaseExpired",
        ),
    ] {
        fixture._leases.set_grant(project.clone(), grant).await;
        let view = fixture
            .api
            .call("view_work", json!({"projectId":project}))
            .await?;
        assert_eq!(
            view.pointer("/result/readyQueue"),
            Some(&json!([task])),
            "{view}"
        );
        assert_eq!(view.pointer("/result/readMark"), None);
        let content = fixture.api.call("feed_read",json!({"projectId":project,"reader":{"kind":"identified","reader":fixture.actor},"after":{"epoch":1,"position":0},"limit":100,"includeRedacted":true})).await?;
        assert_eq!(
            content.pointer("/result/readMark/kind"),
            Some(&json!("notAdvanced"))
        );
        assert_eq!(
            content.pointer("/result/readMark/reason/kind"),
            Some(&json!(reason))
        );
        let mut snapshot = store.snapshot().await?;
        assert_eq!(snapshot.high_water(), high_water);
        assert!(snapshot.records(RecordKind::ReadMark).await?.is_empty());
        snapshot.finish().await?;
    }
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn filtered_windows_and_completed_decision_ranges_advance_only_their_covered_positions()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, workstream) = fixture.project().await?;
    for text in ["first decision", "second decision"] {
        accepted(&fixture.api.call("work_trail_add",fixture.request(&project,json!({"kind":"workTrailAdd","rowId":WorkTrailRowId::generate(),"subject":{"kind":"workstream","subject":workstream},"text":text,"tags":["decision"],"links":[]}))).await?);
    }
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project missing")?;
    let mut snapshot = store.snapshot().await?;
    let initial_high = snapshot.high_water();
    let ledgers = snapshot.records(RecordKind::Request).await?.len();
    snapshot.finish().await?;
    let reader: Identity = serde_json::from_value(fixture.actor.clone())?;
    let declared = json!({"kind":"identified","reader":reader});
    let scope = ReadScope::Project {
        project_id: project.clone(),
    };
    let request = json!({"projectId":project,"selection":{"kind":"forward","after":{"epoch":1,"position":0}},"limit":2,"subject":null,"tags":["never-recorded"],"includeRedacted":false,"reader":declared});
    let empty = fixture.api.call("work_trail_show", request).await?;
    assert_eq!(
        empty.pointer("/result/value/page/rows"),
        Some(&json!([])),
        "{empty}"
    );
    assert_eq!(
        empty.pointer("/result/value/page/coveredThrough"),
        Some(&json!({"epoch":1,"position":2}))
    );
    assert_eq!(
        empty.pointer("/result/readMark"),
        Some(&json!({"kind":"advanced","through":{"epoch":1,"position":2}}))
    );
    let mut snapshot = store.snapshot().await?;
    let mark = snapshot
        .read_mark(&reader, &scope)
        .await?
        .ok_or("mark missing")?;
    assert_eq!(mark.through.position, FeedPosition::try_from(2)?);
    assert_eq!(mark.envelope.actor.declared_identity(), Some(&reader));
    assert!(mark.envelope.request.is_none());
    assert_eq!(
        snapshot.high_water(),
        initial_high,
        "marking must allocate no activity"
    );
    assert_eq!(
        snapshot.records(RecordKind::Request).await?.len(),
        ledgers,
        "marking must allocate no ledger"
    );
    snapshot.finish().await?;
    let register = json!({"projectId":project,"selection":{"kind":"decisions","after":null,"before":null,"rangeTop":null},"limit":1,"subject":null,"tags":[],"includeRedacted":false,"reader":declared});
    let first = fixture
        .api
        .call("work_trail_show", register.clone())
        .await?;
    assert_eq!(
        first
            .pointer("/result/value/page/rows")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(
        first.pointer("/result/value/page/coveredThrough"),
        Some(&Value::Null)
    );
    assert_eq!(
        first.pointer("/result/readMark"),
        Some(&json!({"kind":"unchanged"}))
    );
    let mut snapshot = store.snapshot().await?;
    assert_eq!(
        snapshot
            .read_mark(&reader, &scope)
            .await?
            .ok_or("mark missing")?
            .through,
        mark.through
    );
    assert_eq!(snapshot.high_water(), initial_high);
    assert_eq!(snapshot.records(RecordKind::Request).await?.len(), ledgers);
    snapshot.finish().await?;
    let mut next = register;
    next["selection"]["before"] = first
        .pointer("/result/value/page/olderThan")
        .cloned()
        .ok_or("olderThan missing")?;
    next["selection"]["rangeTop"] = first
        .pointer("/result/value/page/rangeTop")
        .cloned()
        .ok_or("rangeTop missing")?;
    let expected: FeedCursor = serde_json::from_value(next["selection"]["rangeTop"].clone())?;
    fixture.task(&project, &workstream).await?;
    let last = fixture.api.call("work_trail_show", next.clone()).await?;
    assert_eq!(
        last.pointer("/result/value/page/coveredThrough"),
        Some(&json!(expected))
    );
    assert_eq!(
        last.pointer("/result/readMark"),
        Some(&json!({"kind":"advanced","through":expected}))
    );
    let replay = fixture.api.call("work_trail_show", next).await?;
    assert_eq!(
        replay.pointer("/result/readMark/kind"),
        Some(&json!("unchanged"))
    );
    let feed = json!({"projectId":project,"after":{"epoch":1,"position":0},"limit":100,"includeRedacted":false,"reader":declared});
    let newer = fixture.api.call("feed_read", feed.clone()).await?;
    let new_top: FeedCursor = serde_json::from_value(
        newer
            .pointer("/result/value/coveredThrough")
            .cloned()
            .ok_or("feed coverage missing")?,
    )?;
    assert!(new_top.position > expected.position);
    assert_eq!(
        newer.pointer("/result/readMark"),
        Some(&json!({"kind":"advanced","through":new_top}))
    );
    assert_eq!(
        fixture
            .api
            .call("feed_read", feed)
            .await?
            .pointer("/result/readMark/kind"),
        Some(&json!("unchanged"))
    );
    let mut snapshot = store.snapshot().await?;
    assert_eq!(
        snapshot
            .read_mark(&reader, &scope)
            .await?
            .ok_or("mark missing")?
            .through,
        new_top
    );
    assert_eq!(snapshot.high_water(), new_top);
    assert_eq!(
        snapshot.records(RecordKind::Request).await?.len(),
        ledgers + 1
    );
    assert_eq!(snapshot.records(RecordKind::ReadMark).await?.len(), 1);
    snapshot.finish().await?;
    fixture.stop().await
}
