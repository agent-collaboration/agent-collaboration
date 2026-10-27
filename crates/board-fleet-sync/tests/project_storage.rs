use message_board::*;
use message_board_storage::{ImportRowKind, ProjectStorageError, ProjectWriteTransaction};
use std::sync::Arc;
#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
mod project_storage_support;
use project_storage_support::*;

#[tokio::test]
async fn commits_reopen_and_rollback_use_the_library_owned_write_and_read_scopes() {
    // Arrange: the database ID is the project ID, with a declared actor on another home Router.
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("project.sqlite");
    let project = ProjectId::generate();
    let peer = peer();
    let source = origin(2, "recording router");
    let (database, store) = open_project(&path, &project, peer).await;
    let record = project_record(&project, &source);
    let mut write = store.begin_admission(source.clone()).await.unwrap();
    let first = write.append_record(record.clone()).await.unwrap();
    write.commit().await.unwrap();
    let mut pinned = store.snapshot().await.unwrap();

    // Act: commit a second revision while the first real read transaction stays pinned.
    let mut write = store.begin_admission(source.clone()).await.unwrap();
    let second = write.append_record(revised(&record, 2)).await.unwrap();
    write.commit().await.unwrap();
    let mut abandoned = store.begin_admission(source.clone()).await.unwrap();
    let rolled_back = abandoned.append_record(revised(&record, 3)).await.unwrap();
    abandoned.rollback().await.unwrap();

    // Assert: the pinned reader stays consistent and rollback leaves no durable gap.
    assert_eq!(first.position.get(), 1);
    assert_eq!(second.position.get(), 2);
    assert_eq!(rolled_back.position.get(), 3);
    assert_eq!(pinned.high_water(), first);
    assert!(
        matches!(pinned.record(&project.clone().into()).await.unwrap(), Some(Record::Project(row)) if row.revision.get()==1)
    );
    pinned.finish().await.unwrap();
    store.close().await;
    drop(database);
    let (_, store) = open_project(&path, &project, peer).await;
    let mut reopened = store.snapshot().await.unwrap();
    assert_eq!(reopened.high_water(), second);
    assert!(
        matches!(reopened.record(&project.clone().into()).await.unwrap(), Some(Record::Project(row)) if row.revision.get()==2)
    );
    let (entries, covered_through) = accepted_entries(
        reopened
            .feed_after(
                FeedCursor {
                    epoch: EpochNumber::INITIAL,
                    position: FeedPosition::ZERO,
                },
                20,
                false,
            )
            .await
            .unwrap(),
    );
    assert_eq!(entries.len(), 2);
    assert_eq!(covered_through, second);
    assert!(
        entries
            .iter()
            .all(|entry| entry.actor == record.envelope().actor && entry.origin == source)
    );
    reopened.finish().await.unwrap();
    store.close().await;
}

#[tokio::test]
async fn full_session_refs_keep_one_mark_and_watch_key_across_origins_and_reopen() {
    // Arrange: a foreign declared reader has one mark and a project watch.
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("project.sqlite");
    let project = ProjectId::generate();
    let peer = peer();
    let source = origin(2, "first enrollment label");
    let (database, store) = open_project(&path, &project, peer).await;
    let record = project_record(&project, &source);
    let mark_id = RecordId::generate();
    let scope = ReadScope::Project {
        project_id: project.clone(),
    };
    let mut write = store.begin_admission(source.clone()).await.unwrap();
    let first = write.append_record(record.clone()).await.unwrap();
    write
        .write_reader_mark(mark(&record, mark_id.clone(), first))
        .await
        .unwrap();
    write
        .set_project_watch(&reader(1), first, true)
        .await
        .unwrap();
    write.commit().await.unwrap();
    let first_watch: String = sqlx::query_scalar("SELECT watch_id FROM project_watches")
        .fetch_one(database.reader())
        .await
        .unwrap();
    store.close().await;
    drop(database);

    // Act: a different admitted origin relabels the enrollment and advances the same reader.
    let (database, store) = open_project(&path, &project, peer).await;
    let later_origin = origin(3, "relabelled enrollment");
    let mut write = store.begin_admission(later_origin.clone()).await.unwrap();
    let second = write.append_record(revised(&record, 2)).await.unwrap();
    write
        .write_reader_mark(mark(&record, mark_id.clone(), second))
        .await
        .unwrap();
    write
        .set_project_watch(&reader(1), second, true)
        .await
        .unwrap();
    write
        .set_project_watch(&reader(4), second, true)
        .await
        .unwrap();
    write.commit().await.unwrap();
    let actors: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM actors")
        .fetch_one(database.reader())
        .await
        .unwrap();
    let retained_watch: String = sqlx::query_scalar("SELECT w.watch_id FROM project_watches w JOIN actors a ON a.actor_id=w.reader_actor WHERE a.canonical_identity=?").bind(serde_json::to_string(&RecordedActor::declared(reader(1))).unwrap()).fetch_one(database.reader()).await.unwrap();
    let mut read = store.snapshot().await.unwrap();
    let advanced = read.read_mark(&reader(1), &scope).await.unwrap().unwrap();

    // Assert: context and labels do not re-key a reader; a different home ref remains distinct.
    assert_eq!(actors, 2);
    assert_eq!(retained_watch, first_watch);
    assert_eq!(advanced.envelope.id, mark_id);
    assert_eq!(advanced.through, second);
    assert_eq!(advanced.envelope.actor, record.envelope().actor);
    assert_eq!(advanced.envelope.position, RecordPlacement::Unsequenced);
    assert!(read.read_mark(&reader(4), &scope).await.unwrap().is_none());
    let (entries, _) = accepted_entries(
        read.feed_after(
            FeedCursor {
                epoch: EpochNumber::INITIAL,
                position: FeedPosition::ZERO,
            },
            20,
            false,
        )
        .await
        .unwrap(),
    );
    assert_eq!(entries.first().unwrap().origin, source);
    assert_eq!(entries.get(1).unwrap().origin, later_origin);
    assert_eq!(
        entries.first().unwrap().actor,
        entries.get(1).unwrap().actor
    );
    assert_eq!(
        read.high_water(),
        second,
        "reader state must not allocate feed positions"
    );
    read.finish().await.unwrap();
    store.close().await;
}

#[tokio::test]
async fn privileged_import_uses_the_exact_supplied_keys_and_keeps_unknown_time() {
    // Arrange: the bounded key source stands in only for the importer's derivation policy.
    let directory = tempfile::tempdir().unwrap();
    let project = ProjectId::generate();
    let source = origin(2, "importing router");
    let (database, store) =
        open_project(&directory.path().join("project.sqlite"), &project, peer()).await;
    let mut record = project_record(&project, &source);
    record.envelope_mut().time = RecordTime::LegacyUnknown;
    let write = database.begin_write().await.unwrap();
    let mut import = ProjectWriteTransaction::for_import(
        project.clone(),
        write,
        fleet_admission::ImportProvenance {
            installation: source.installation.clone(),
            machine: source.machine.clone(),
        },
        Arc::new(FixedImportKeys {
            invalid_actor: false,
        }),
    )
    .await
    .unwrap();

    // Act: execute the real typed insert and commit through the actual library scope.
    let cursor = import.append_record(record.clone()).await.unwrap();
    import.commit().await.unwrap();
    let actual: (String, String, String) = sqlx::query_as("SELECT a.actor_id,e.line_id,f.entry_id FROM actors a CROSS JOIN epoch_lines e CROSS JOIN project_feed f").fetch_one(database.reader()).await.unwrap();
    let mut read = store.snapshot().await.unwrap();
    let restored = read.record(&project.clone().into()).await.unwrap().unwrap();

    // Assert: storage validates supplied physical keys without re-deriving or replacing them.
    assert_eq!(
        actual,
        (
            ACTOR_KEY.to_owned(),
            LINE_KEY.to_owned(),
            FEED_KEY.to_owned()
        )
    );
    assert_eq!(restored.envelope().time, RecordTime::LegacyUnknown);
    assert_eq!(restored.envelope().actor, record.envelope().actor);
    assert_eq!(restored.envelope().position.origin(), Some(&source));
    assert_eq!(read.high_water(), cursor);
    read.finish().await.unwrap();
    store.close().await;
}

#[tokio::test]
async fn an_invalid_import_key_is_refused_without_any_committed_project_effect() {
    let directory = tempfile::tempdir().unwrap();
    let project = ProjectId::generate();
    let source = origin(2, "importing router");
    let (database, store) =
        open_project(&directory.path().join("project.sqlite"), &project, peer()).await;
    let mut record = project_record(&project, &source);
    record.envelope_mut().time = RecordTime::LegacyUnknown;
    let mut import = ProjectWriteTransaction::for_import(
        project.clone(),
        database.begin_write().await.unwrap(),
        fleet_admission::ImportProvenance {
            installation: source.installation.clone(),
            machine: source.machine.clone(),
        },
        Arc::new(FixedImportKeys {
            invalid_actor: true,
        }),
    )
    .await
    .unwrap();
    assert!(matches!(
        import.append_record(record).await,
        Err(ProjectStorageError::InvalidImportRowKey {
            kind: ImportRowKind::Actor
        })
    ));
    import.rollback().await.unwrap();
    let count: i64 = sqlx::query_scalar("SELECT (SELECT COUNT(*) FROM project_state)+(SELECT COUNT(*) FROM actors)+(SELECT COUNT(*) FROM epoch_lines)+(SELECT COUNT(*) FROM project_feed)").fetch_one(database.reader()).await.unwrap();
    assert_eq!(count, 0);
    store.close().await;
}

#[tokio::test]
async fn a_router_actor_mismatch_is_refused_before_any_project_record_is_committed() {
    let directory = tempfile::tempdir().unwrap();
    let project = ProjectId::generate();
    let source = origin(2, "recording router");
    let (database, store) =
        open_project(&directory.path().join("project.sqlite"), &project, peer()).await;
    let mut record = project_record(&project, &source);
    record.envelope_mut().actor = RecordedActor::Router {
        component: RouterComponent::SubscriptionDelivery,
        installation: origin(1, "other router").installation,
    };
    let mut write = store.begin_admission(source).await.unwrap();
    assert!(matches!(
        write.append_record(record).await,
        Err(ProjectStorageError::InvalidRecord {
            field: "actor.installation",
            ..
        })
    ));
    write.rollback().await.unwrap();
    let count:i64=sqlx::query_scalar("SELECT (SELECT COUNT(*) FROM actors)+(SELECT COUNT(*) FROM project_state)+(SELECT COUNT(*) FROM project_feed)").fetch_one(database.reader()).await.unwrap();
    assert_eq!(count, 0);
    store.close().await;
}

#[tokio::test]
async fn a_typed_child_write_cannot_silently_replace_a_false_workspace_scope() {
    let directory = tempfile::tempdir().unwrap();
    let project = ProjectId::generate();
    let source = origin(2, "recording router");
    let (database, store) =
        open_project(&directory.path().join("project.sqlite"), &project, peer()).await;
    let record = project_record(&project, &source);
    let mut initial = store.begin_admission(source.clone()).await.unwrap();
    initial.append_record(record.clone()).await.unwrap();
    initial.commit().await.unwrap();
    let id = WorkstreamId::generate();
    let mut envelope = record.envelope().clone();
    envelope.id = id.clone().into();
    envelope.scope.workspace = WorkspaceId::generate();
    let child = Record::Workstream(Workstream {
        envelope,
        workstream_id: id,
        project_id: project,
        name: ResourceName::try_from("child".to_owned()).unwrap(),
        goal: "scope proof".into(),
        state: WorkstreamState::Open {},
        revision: SubjectRevision::INITIAL,
    });
    let mut write = store.begin_admission(source).await.unwrap();
    assert!(matches!(
        write.append_record(child).await,
        Err(ProjectStorageError::InvalidRecord { field: "scope", .. })
    ));
    write.rollback().await.unwrap();
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM workstreams")
        .fetch_one(database.reader())
        .await
        .unwrap();
    assert_eq!(count, 0);
    store.close().await;
}

#[test]
fn normal_project_requests_cannot_select_the_privileged_import_key_source() {
    let project = ProjectId::generate();
    let request = ProjectRequest {
        request_id: RequestId::generate(),
        project_id: project.clone(),
        actor: reader(1),
        acting_for: None,
        protocol_version: ProtocolVersion::CURRENT,
        operation: BoardOperation::ReadMarkAdvance {
            reader: reader(1),
            scope: ReadScope::Project {
                project_id: project,
            },
            through: FeedCursor {
                epoch: EpochNumber::INITIAL,
                position: FeedPosition::ZERO,
            },
        },
    };
    let encoded = serde_json::to_value(request).unwrap();
    assert!(serde_json::from_value::<ProjectRequest>(encoded.clone()).is_ok());
    for field in ["importRowKeys", "importMode"] {
        let mut supplied = encoded.clone();
        supplied[field] = serde_json::json!(true);
        assert!(serde_json::from_value::<ProjectRequest>(supplied).is_err());
    }
}
