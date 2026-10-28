//! FL-5 / decision1200: a real holder records ArchiveSeen, replicates it, then reports.
use super::*;
use board_central_test_support::{DispatchFaultObservation, ObjectAddress, RuntimeControl};
use futures_util::FutureExt;
use std::panic::{AssertUnwindSafe, resume_unwind};

type ArchiveResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

async fn observed_fact(
    store: &collaboration_runtime::NativeProjectStore,
    board: &BoardId,
) -> ArchiveResult<Fact> {
    // FL-6 permits a missed/coalesced hint. Allow PD3's connected Fleet repair
    // interval plus the ordinary operation bound without prompting the producer.
    let observation_bound = super::super::machine_session::FLEET_REFRESH_PERIOD + BOUND;
    timeout(observation_bound, async {
        loop {
            let mut read = store.snapshot().await?;
            let facts = read.records(RecordKind::Fact).await?;
            read.finish().await?;
            let archive = facts.into_iter().filter_map(|record| match record {
                Record::Fact(fact) if matches!(&fact.kind,
                    FactKind::Lifecycle(LifecycleFact::ArchiveSeen { board: observed }) if observed == board) => Some(fact),
                _ => None,
            }).collect::<Vec<_>>();
            if !archive.is_empty() {
                assert_eq!(archive.len(), 1, "archive observation must be idempotent");
                return archive.into_iter().next().ok_or_else(|| "archive fact missing".into());
            }
            tokio::task::yield_now().await;
        }
    }).await.map_err(|error| format!("accepted BoardArchive produced no local ArchiveSeen fact: {error}"))?
}

async fn archive_with_held_copy(fixture: &claim_fixture::ClaimFixture) -> TestResult {
    let project = ProjectId::generate();
    project_claim::imported_snapshot(
        &fixture.central,
        &fixture.directory.path().join("source.sqlite"),
        &project,
    )
    .await?;
    assert!(matches!(
        fixture.claim(&project).await?,
        board_sync_protocol::ProjectClaimReply::Claimed { .. }
    ));
    let store = fixture
        .registry
        .existing_project(&project)
        .await?
        .ok_or("held project")?;
    let mut read = store.snapshot().await?;
    let Some(Record::Project(state)) = read.record(&project.clone().into()).await? else {
        return Err("project state".into());
    };
    let head = read.high_water();
    read.finish().await?;
    let board = state.envelope.scope.board.ok_or("project board")?;
    let fleet = fixture
        .fleet
        .upgrade()
        .ok_or("fixture Fleet owner absent")?;
    fleet.synchronize().await?;
    timeout(BOUND, async {
        loop {
            if fixture
                .projects
                .replicated_through(&project, head.epoch)
                .await?
                .position
                >= head.position
            {
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await??;
    let connection = ObjectAddress::Connection(origin()?.installation);
    let copy = ObjectAddress::Copy(fleet_sync_protocol::DatabaseId::try_from(
        project.as_str().to_owned(),
    )?);
    fixture
        .central
        .runtime_control(
            connection.clone(),
            RuntimeControl::HoldNextDispatch {
                destination: copy.clone(),
            },
        )
        .await?;
    let application = (collaboration_service::ApplicationFixture::new(
        origin()?.installation.as_str(),
        &uuid::Uuid::now_v7().to_string(),
    )?
    .with_project_admission(fixture.admission.clone()))
    .build();
    let reply = application
        .board()
        .board_archive(serde_json::from_value(serde_json::json!({
        "requestId":RequestId::generate(),"protocolVersion":1,"actingFor":null,
        "actor":{"kind":"human","humanId":"archive-producer-owner"},"boardId":board}))?)
        .await?;
    assert!(matches!(
        reply.result,
        message_board::public_replies::RequestResult::Decided(
            message_board::public_replies::RequestDecision::Accepted { .. }
        )
    ));
    // Only the real hint or periodic repair may produce this fact; a test call
    // cannot conceal missing automated producer wiring.
    let fact = observed_fact(&store, &board).await?;
    assert_eq!(
        serde_json::to_value(&fact.envelope.actor)?,
        serde_json::json!({
        "kind":"router","component":"archiveObservation","installation":origin()?.installation})
    );
    assert!(fact.envelope.request.is_none());
    assert!(matches!(fact.envelope.time, RecordTime::Router(_)));
    let cursor = fact
        .envelope
        .position
        .project_cursor()
        .ok_or("archive fact cursor")?;
    let held = timeout(
        BOUND,
        fixture
            .central
            .runtime_control(connection.clone(), RuntimeControl::AwaitHeldDispatch),
    )
    .await??;
    assert!(
        matches!(held.dispatch, DispatchFaultObservation::Held { destination, .. } if destination == copy)
    );
    let through = fixture
        .projects
        .replicated_through(&project, cursor.epoch)
        .await?;
    assert!(
        through.position < cursor.position,
        "selected Copy hold did not precede this fact's acknowledgement"
    );
    fleet.synchronize().await?;
    assert!(
        matches!(fleet.record(&board.clone().into()).await?,
        Some(Record::Board(value)) if value.state == BoardState::Archiving),
        "board finalized before its held archive fact was acknowledged"
    );
    let repeated = fixture
        .admission
        .observe_archiving_project(&project)
        .await?
        .ok_or("existing observation")?;
    assert_eq!(repeated.position, cursor);
    assert_eq!(observed_fact(&store, &board).await?.fact_id, fact.fact_id);
    fixture
        .central
        .runtime_control(connection, RuntimeControl::ResumeDispatch)
        .await?;
    timeout(BOUND, async {
        loop {
            let through = fixture
                .projects
                .replicated_through(&project, cursor.epoch)
                .await?;
            if through.epoch == cursor.epoch && through.position >= cursor.position {
                fleet.synchronize().await?;
                if matches!(fleet.record(&board.clone().into()).await?,
                    Some(Record::Board(value)) if value.state == BoardState::Archived)
                {
                    return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
                }
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(|error| {
        format!("ArchiveSeen exists but acknowledged board completion did not arrive: {error}")
    })??;
    Ok(())
}

#[tokio::test]
async fn archiving_a_held_project_records_its_own_fact_then_finalizes_the_board() -> TestResult {
    // Wrong builds: absent producer, duplicate local fact, or a report before native Copy ACK.
    let fixture = Box::pin(claim_fixture::ClaimFixture::start()).await?;
    let proof = AssertUnwindSafe(archive_with_held_copy(&fixture))
        .catch_unwind()
        .await;
    let connection = ObjectAddress::Connection(origin()?.installation);
    let released = match fixture
        .central
        .runtime_control(connection.clone(), RuntimeControl::Observe)
        .await
    {
        Ok(observed) => match observed.dispatch {
            DispatchFaultObservation::Held { .. } => fixture
                .central
                .runtime_control(connection, RuntimeControl::ResumeDispatch)
                .await
                .map(|_| ()),
            DispatchFaultObservation::Idle | DispatchFaultObservation::Armed { .. } => Ok(()),
        },
        Err(error) => Err(error),
    };
    let closed = fixture.shutdown().await;
    let cleanup = match (released, closed) {
        (Ok(()), result) | (result, Ok(())) => result,
        (Err(release), Err(close)) => {
            Err(format!("held dispatch release: {release}; fixture shutdown: {close}").into())
        }
    };
    match proof {
        Ok(result) => match (result, cleanup) {
            (Ok(()), result) | (result, Ok(())) => result,
            (Err(proof), Err(cleanup)) => {
                Err(format!("archive proof: {proof}; cleanup: {cleanup}").into())
            }
        },
        Err(panic) => {
            if let Err(error) = cleanup {
                eprintln!("archive producer cleanup also failed: {error}");
            }
            resume_unwind(panic)
        }
    }
}
