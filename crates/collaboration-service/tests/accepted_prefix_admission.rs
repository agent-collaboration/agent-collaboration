//! Consumer admission keeps accepted older cursors while assigning the actual current epoch.
#[path = "support/accepted_snapshot.rs"]
pub mod accepted_snapshot;
#[path = "support/board_fixture.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
#[expect(
    clippy::indexing_slicing,
    reason = "This test consumes a subset of the shared fixture and retains its fixed JSON construction."
)]
pub mod board_fixture;
use board_fixture::{BoardFixture, TestError, accepted};
use message_board::*;
use serde_json::{Value, json};

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn a_bootstrapped_file_reopened_as_provisional_is_superseded() -> Result<(), TestError> {
    let source = BoardFixture::start().await?;
    let (project, _) = source.project().await?;
    let target = BoardFixture::start().await?;
    let (_, epoch) = accepted_snapshot::bootstrap_project(&source, &target, &project).await?;
    let database_id =
        fleet_sync_protocol::DatabaseId::new(uuid::Uuid::parse_str(project.as_str())?)?;
    let path = target
        ._directory
        .path()
        .join("projects")
        .join(project.as_str())
        .join(format!("e{}.sqlite", epoch.get()));
    let database = fleet_sync_writer::ReplicatedDatabase::open(
        &path,
        database_id,
        board_fleet_sync::BoardProjectSchema::new()?,
        fleet_sync_lease::LeaseHolder::provisional(
            database_id,
            board_fixture::fixture_peer()?,
            board_fleet_sync::BOARD_LEASE_POLICY,
        ),
    )
    .await?;
    assert!(matches!(database.writer_state().await?,
        fleet_sync_lease::WriterState::Superseded { current } if current.value() == epoch.get()));
    database.close().await;
    target.stop().await?;
    source.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn a_filename_epoch_that_differs_from_the_native_file_is_refused() -> Result<(), TestError> {
    let source = BoardFixture::start().await?;
    let (project, _) = source.project().await?;
    let target = BoardFixture::start().await?;
    let (_, epoch) = accepted_snapshot::bootstrap_project(&source, &target, &project).await?;
    let directory = target
        ._directory
        .path()
        .join("projects")
        .join(project.as_str());
    // Every writer is closed; only this isolated fixture's filename is changed.
    tokio::fs::rename(
        directory.join(format!("e{}.sqlite", epoch.get())),
        directory.join(format!("e{}.sqlite", epoch.get() + 1)),
    )
    .await?;
    match target._registry.existing_project(&project).await {
        Err(collaboration_runtime::ProjectRegistryError::Storage(
            message_board_storage::ProjectStorageError::Writer(
                fleet_sync_writer::OpenError::Corrupt { field },
            ),
        )) => assert_eq!(field, "writer.epoch"),
        Err(error) => return Err(format!("unexpected mismatch refusal: {error}").into()),
        Ok(_) => return Err("a filename epoch granted access to a mismatched native file".into()),
    }
    target.stop().await?;
    source.stop().await
}

fn current_request(
    fixture: &BoardFixture,
    project: &ProjectId,
    _epoch: EpochNumber,
    operation: Value,
) -> Result<Value, TestError> {
    let request: ProjectRequest = serde_json::from_value(fixture.request(project, operation))?;
    Ok(serde_json::to_value(request)?)
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn cold_metadata_from_another_project_is_refused_without_rewriting_the_file()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, _) = fixture.project().await?;
    let directory = fixture.stop_and_keep_directory().await?;
    let other = ProjectId::generate();
    let projects = directory.path().join("projects");
    tokio::fs::rename(
        projects.join(project.as_str()),
        projects.join(other.as_str()),
    )
    .await?;
    let path = projects.join(other.as_str()).join("e1.sqlite");
    let before = tokio::fs::read(&path).await?;
    let registry = collaboration_runtime::ProjectStoreRegistry::new(
        directory.path(),
        Some(board_fixture::fixture_peer()?),
        std::sync::Arc::new(runtime_primitives::SystemClock),
    );
    match registry.existing_project(&other).await {
        Err(collaboration_runtime::ProjectRegistryError::Storage(
            message_board_storage::ProjectStorageError::Writer(
                fleet_sync_writer::OpenError::Corrupt { field },
            ),
        )) => assert_eq!(field, "writer.database"),
        Err(error) => return Err(format!("unexpected identity refusal: {error}").into()),
        Ok(_) => return Err("cold metadata accepted another project's identity".into()),
    }
    assert_eq!(tokio::fs::read(&path).await?, before);
    registry.close().await?;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn inherited_cursors_keep_marks_and_late_anchors_while_new_close_coordinates_use_the_admitted_epoch()
-> Result<(), TestError> {
    let source = BoardFixture::start().await?;
    let (project, _) = source.project().await?;
    let target = BoardFixture::start().await?;
    let (inherited, epoch) =
        accepted_snapshot::bootstrap_project(&source, &target, &project).await?;
    let store = target
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("seeded project")?;
    let state = store.writer_state().await?;
    assert!(
        matches!(&state, fleet_sync_lease::WriterState::Writing { epoch: actual } if actual.value() == epoch.get()),
        "restored writer state: {state:?}"
    );
    let reader: Identity = serde_json::from_value(target.actor.clone())?;
    assert_eq!(
        target
            ._admission
            .mark_covered_read(
                &project,
                &reader,
                ReadScope::Project {
                    project_id: project.clone()
                },
                inherited
            )
            .await?,
        ReadMarkOutcome::Advanced { through: inherited }
    );
    let mut read = store.snapshot().await?;
    assert_eq!(
        read.high_water(),
        inherited,
        "a current-epoch bookkeeping write invented a new feed head"
    );
    let Some(Record::Project(state)) = read.record(&project.clone().into()).await? else {
        return Err("project state".into());
    };
    read.finish().await?;
    let close = target.api.call("project_close", current_request(&target, &project, epoch,
        json!({"kind":"projectClose","reason":"finished","expectedRevision":state.revision}))?).await?;
    accepted(&close);
    let mut read = store.snapshot().await?;
    let Some(Record::Project(state)) = read.record(&project.clone().into()).await? else {
        return Err("closed project".into());
    };
    let closed_at = state.closed_at.ok_or("close coordinate")?;
    assert_eq!(closed_at.epoch, epoch);
    assert_eq!(closed_at.position.get(), inherited.position.get() + 2);
    assert!(matches!(
        read.cursor_validity(closed_at).await?,
        message_board_storage::CursorValidity::Valid
    ));
    read.finish().await?;
    let fact_id = FactId::generate();
    let fact = target.api.call("fact_record", current_request(&target, &project, epoch,
        json!({"kind":"factRecord","fact":{"factId":fact_id,"kind":{"kind":"evaluatorResult","fact":{"guard":"accepted evidence","subject":{"kind":"project","id":project},"outcome":"met","evidence":[],"evaluator":{"actor":RecordedActor::declared(reader.clone()),"engine":null,"calibration":null},"executor":null}},"links":[{"kind":"project","id":project}],"producerTime":"2026-10-12T00:00:00Z","anchor":inherited}}))?).await?;
    accepted(&fact);
    let mark_request: ProjectRequest = serde_json::from_value(current_request(
        &target,
        &project,
        epoch,
        json!({"kind":"readMarkAdvance","reader":reader,"scope":{"kind":"project","projectId":project},"through":inherited}),
    )?)?;
    let mark = target._admission.submit(&mark_request).await?;
    assert!(matches!(
        mark.result,
        RequestResult::Decided(RequestDecision::Accepted {
            change: ChangeOutcome::AlreadySo,
            ..
        })
    ));
    let mut read = store.snapshot().await?;
    let Some(Record::Fact(fact)) = read.record(&fact_id.into()).await? else {
        return Err("late fact".into());
    };
    assert!(fact.late);
    assert_eq!(fact.anchor, Some(inherited));
    assert_eq!(
        fact.envelope
            .position
            .project_cursor()
            .ok_or("fact coordinate")?
            .epoch,
        epoch
    );
    assert_eq!(
        read.read_mark(
            &reader,
            &ReadScope::Project {
                project_id: project.clone()
            }
        )
        .await?
        .ok_or("mark")?
        .through,
        inherited
    );
    read.finish().await?;
    drop(store);
    target.stop().await?;
    source.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn a_current_epoch_acknowledgement_covers_an_inherited_stand_down_for_the_exact_reader()
-> Result<(), TestError> {
    let source = BoardFixture::start().await?;
    let (project, workstream) = source.project().await?;
    let reader = Identity::Human {
        human_id: HumanId::try_from("stood down reader".to_owned())?,
    };
    for (holder, revision) in [
        (reader.clone(), 0),
        (
            Identity::Human {
                human_id: HumanId::try_from("replacement".to_owned())?,
            },
            1,
        ),
    ] {
        accepted(&source.api.call("role_grant", source.request(&project, json!({"kind":"roleGrant","subject":{"kind":"workstream","subject":workstream},"role":"implementer","holder":holder,"model":null,"lineage":null,"expectedRevision":revision,"ownerGrant":null}))).await?);
    }
    let target = BoardFixture::start().await?;
    let (inherited, epoch) =
        accepted_snapshot::bootstrap_project(&source, &target, &project).await?;
    let task = current_request(
        &target,
        &project,
        epoch,
        json!({"kind":"taskCreate","taskId":TaskId::generate(),"workstreamId":workstream,"title":"current activity","body":"","blockedBy":[],"relatedTo":[]}),
    )?;
    accepted(&target.api.call("task_create", task).await?);
    let store = target
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("seeded project")?;
    let read = store.snapshot().await?;
    let covered = read.high_water();
    read.finish().await?;
    assert_eq!(covered.epoch, epoch);
    assert!(covered.position > inherited.position);
    let mut ack = current_request(
        &target,
        &project,
        epoch,
        json!({"kind":"roleAcknowledgeCurrent","readThrough":covered}),
    )?;
    ack["actor"] = json!(reader);
    accepted(&target.api.call("role_acknowledge_current", ack).await?);
    assert_eq!(
        target
            ._admission
            .mark_covered_read(
                &project,
                &reader,
                ReadScope::Project {
                    project_id: project.clone()
                },
                covered
            )
            .await?,
        ReadMarkOutcome::Advanced { through: covered }
    );
    let mut read = store.snapshot().await?;
    assert_eq!(
        read.read_mark(
            &reader,
            &ReadScope::Project {
                project_id: project.clone()
            }
        )
        .await?
        .ok_or("acknowledged read mark")?
        .through,
        covered
    );
    read.finish().await?;
    drop(store);
    target.stop().await?;
    source.stop().await
}
