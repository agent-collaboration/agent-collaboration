//! Identified reads mark coverage without keys, ledgers or activity positions.
#[path = "support/board_fixture.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
pub mod board_fixture;
use board_fixture::{BoardFixture, TestError, accepted};
use message_board::*;
use serde_json::{Value, json};
use sqlx::{ConnectOptions, Connection};

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn too_new_schema_is_refused_unchanged_and_mismatched_schema_keeps_readable_data()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let reader: Identity = serde_json::from_value(fixture.actor.clone())?;
    for reason in ["tooNew", "mismatch"] {
        let project = ProjectId::generate();
        let workstream = WorkstreamId::generate();
        let seed_registry =
            board_fixture::native_registry::new_registry(fixture._directory.path())?;
        let input: ProjectCreateRequest = serde_json::from_value(json!({
            "requestId":RequestId::generate(),"protocolVersion":1,"actor":reader,
            "projectId":project,"boardId":fixture.board,"title":project.as_str()
        }))?;
        board_fixture::registered_project::prepare_registered_creation(
            &fixture._fleet,
            &seed_registry,
            &input,
            &fixture.record_origin()?,
            &runtime_primitives::SystemClock,
        )
        .await?;
        let seed = seed_registry.open_project(&project).await?;
        let path = seed.path().to_owned();
        let origin = ActorOrigin {
            installation: ServiceId::try_from("00000000-0000-4000-8000-000000000001".to_owned())?,
            machine: MachineLabel::try_from("schema fixture".to_owned())?,
        };
        let envelope = RecordEnvelope {
            id: project.clone().into(),
            scope: RecordScope {
                workspace: fixture.workspace.clone(),
                board: Some(fixture.board.clone()),
                project: Some(project.clone()),
            },
            actor: RecordedActor::declared(reader.clone()),
            request: None,
            position: RecordPlacement::Project(ProjectFeedEntry {
                cursor: FeedCursor {
                    epoch: EpochNumber::try_from(1)?,
                    position: FeedPosition::ZERO,
                },
                origin: origin.clone(),
            }),
            time: RecordTime::Router(chrono::Utc::now()),
        };
        let mut write = seed.begin_admission(origin).await?;
        write
            .append_record(Record::Project(ProjectState {
                envelope: envelope.clone(),
                project_id: project.clone(),
                revision: SubjectRevision::INITIAL,
                closed_at: None,
                description: Description::try_from(String::new())?,
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
        drop(seed);
        std::sync::Arc::try_unwrap(seed_registry)
            .map_err(|_| "seed registry still owned")?
            .close()
            .await?;
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
        if reason == "tooNew" {
            let bytes = std::fs::read(&path)?;
            let modified = std::fs::metadata(&path)?.modified()?;
            match fixture._registry.existing_project(&project).await {
                Err(collaboration_runtime::ProjectRegistryError::Storage(
                    message_board_storage::ProjectStorageError::Writer(
                        fleet_sync_writer::OpenError::SchemaUnsupported {
                            form: fleet_sync_writer::SchemaForm::SchemaTooNew { .. },
                        },
                    ),
                )) => {}
                Err(error) => {
                    return Err(format!("unexpected future-schema refusal: {error}").into());
                }
                Ok(_) => return Err("future schema exposed a raw reader".into()),
            }
            assert_eq!(std::fs::read(&path)?, bytes);
            assert_eq!(std::fs::metadata(&path)?.modified()?, modified);
            continue;
        }
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
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn an_expired_project_still_returns_the_read_and_never_allocates_a_mark()
-> Result<(), TestError> {
    use fleet_sync_protocol::{
        DatabaseId, Epoch, LeaseState, LeaseStatus, RenewAnswer, RenewResult,
    };
    use runtime_primitives::{Clock, SyncTime, SystemClock};
    for scenario in ["pastExpiry", "pastDeadline"] {
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
        let now = SystemClock.now();
        let started = SyncTime::from_unix_millis(now.unix_millis() - 120_000);
        let database = fixture._registry.database(&project).await?;
        let lease = LeaseStatus {
            database: DatabaseId::new(uuid::Uuid::parse_str(project.as_str())?)?,
            epoch: Epoch::new(1)?,
            holder: board_fixture::fixture_peer()?,
            state: LeaseState::Held,
            expires_at: if scenario == "pastExpiry" {
                SyncTime::from_unix_millis(started.unix_millis() + 60_000)
            } else {
                now.after(std::time::Duration::from_secs(3600))
            },
            remaining_ms: 60_000,
        };
        database
            .observe_renewal(
                RenewAnswer::Renewed {
                    recorded: RenewResult {
                        epoch: lease.epoch,
                        expires_at: lease.expires_at,
                    },
                    current: lease,
                    transfer_in_progress: None,
                    replayed: false,
                },
                started,
            )
            .await?;
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
            Some(&json!("leaseExpired"))
        );
        let mut snapshot = store.snapshot().await?;
        assert_eq!(snapshot.high_water(), high_water);
        assert!(snapshot.records(RecordKind::ReadMark).await?.is_empty());
        snapshot.finish().await?;
        drop(database);
        drop(store);
        fixture.stop().await?;
    }
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn superseded_project_reads_are_unavailable_and_allocate_no_mark() -> Result<(), TestError> {
    use runtime_primitives::Clock;
    let fixture = BoardFixture::start().await?;
    let (project, _) = fixture.project().await?;
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let snapshot = store.snapshot().await?;
    let high_water = snapshot.high_water();
    snapshot.finish().await?;
    fixture
        ._registry
        .database(&project)
        .await?
        .observe_renewal(
            fleet_sync_protocol::RenewAnswer::Refused(
                fleet_sync_protocol::LeaseRefusal::NotWriter {
                    current: Some(fleet_sync_protocol::Epoch::new(2)?),
                },
            ),
            runtime_primitives::SystemClock.now(),
        )
        .await?;
    for (route, request) in [
        ("view_work", json!({"projectId":project})),
        (
            "feed_read",
            json!({"projectId":project,"reader":{"kind":"identified","reader":fixture.actor},"after":{"epoch":1,"position":0},"limit":100,"includeRedacted":true}),
        ),
    ] {
        let response = fixture.api.call(route, request).await?;
        assert_eq!(
            response.pointer("/error/data/kind"),
            Some(&json!("boardUnavailable")),
            "{route}: {response}"
        );
        assert!(response.get("result").is_none(), "{response}");
    }
    // Diagnostic access is not a capability exposed by the public read path.
    let mut snapshot = store.snapshot().await?;
    assert_eq!(snapshot.high_water(), high_water);
    assert!(snapshot.records(RecordKind::ReadMark).await?.is_empty());
    snapshot.finish().await?;
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
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
    assert_eq!(
        mark.envelope.actor.declared_identity().as_ref(),
        Some(&reader)
    );
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
