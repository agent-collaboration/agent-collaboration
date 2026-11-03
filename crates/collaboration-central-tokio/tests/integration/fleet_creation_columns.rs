//! Creation bindings retain accepted and rejected identities without requiring directory rows.
use super::composed_fleet_fixture::*;
use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use fleet_admission::{FleetOperation, FleetRequest, FleetRequestResult, ProjectCreationDecision};
use message_board::{ProjectId, RequestDecision, StoredDecision};
use serde_json::{Value, json};
use sqlx_core::{query::query, row::Row};

const BOARD: &str = "019f0000-0000-7000-8000-000000008182";

fn creation(project: ProjectId, board: &str) -> TestResult<FleetRequest> {
    Ok(FleetRequest {
        request_id: message_board::RequestId::generate(),
        workspace: "019f0000-0000-7000-8000-000000008181"
            .to_owned()
            .try_into()?,
        actor: message_board::RecordedActor::Human {
            human_id: "fixture owner".to_owned().try_into()?,
        },
        acting_for: None,
        protocol_version: message_board::ProtocolVersion::CURRENT,
        operation: FleetOperation::ProjectDirectoryCreate {
            project,
            board: board.to_owned().try_into()?,
            title: "immutable creation".to_owned().try_into()?,
        },
    })
}
async fn submit(fixture: &Fixture, request: &FleetRequest) -> TestResult<FleetRequestResult> {
    Ok(serde_json::from_value(fixture.call("session", "central.fleet-metadata", Value::Null, json!({"origin":{"installation":SERVICE_A,"machine":"composed fixture"},"request":request})).await?)?)
}
async fn binding(fixture: &Fixture, project: &ProjectId) -> TestResult<ProjectCreationDecision> {
    let bytes = fixture
        .runtime
        .on_call(serde_json::to_vec(&json!({
            "source":{"kind":"fleet"},"sourceFace":"registry","authority":{"kind":"object"},
            "destination":{"kind":"fleet"},"destinationFace":"fleet",
            "identity":{"operation":durable_steps::Uuid::now_v7(),"step":"creation_read"},
            "kind":"central.creation-decision","target":null,"request":project
        }))?)
        .await
        .map_err(|error| format!("creation read: {error:?}"))?;
    serde_json::from_slice::<Option<ProjectCreationDecision>>(&bytes)?
        .ok_or_else(|| "creation binding absent".into())
}
async fn columns(fixture: &Fixture) -> TestResult<Vec<(String, String, String, i64, i64)>> {
    Ok(fixture.store.transaction(|scope| Box::pin(async move {
        query::<sqlx_turso::Turso>("SELECT project_id,creating_request,carrier_installation,created_at,updated_at FROM fleet_creation_decisions ORDER BY project_id")
            .fetch_all(scope.executor()).await.map_err(StoreError::backend)?.into_iter().map(|row|Ok((row.try_get("project_id").map_err(StoreError::backend)?,row.try_get("creating_request").map_err(StoreError::backend)?,row.try_get("carrier_installation").map_err(StoreError::backend)?,row.try_get("created_at").map_err(StoreError::backend)?,row.try_get("updated_at").map_err(StoreError::backend)?))).collect::<Result<Vec<_>,StoreError>>()
    })).await?)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Accepted and rejected bindings must keep exact injected times and immutable request identity after replay and reopen."
)]
#[tokio::test]
async fn rejected_creation_has_no_directory_row_and_both_creation_bindings_survive_replay_and_reopen()
-> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    fixture.arrange_registered().await?;
    fixture.io.advance(11_000);
    let accepted_project = ProjectId::generate();
    let rejected_project = ProjectId::generate();
    let accepted = creation(accepted_project.clone(), BOARD)?;
    let rejected = creation(
        rejected_project.clone(),
        "019f0000-0000-7000-8000-000000008299",
    )?;
    let accepted_answer = submit(&fixture, &accepted).await?;
    let rejected_answer = submit(&fixture, &rejected).await?;
    assert!(matches!(
        accepted_answer,
        FleetRequestResult::Decided(RequestDecision::Accepted { .. })
    ));
    assert!(matches!(
        rejected_answer,
        FleetRequestResult::Decided(RequestDecision::Rejected { .. })
    ));
    let saved_accepted = binding(&fixture, &accepted_project).await?;
    let saved_rejected = binding(&fixture, &rejected_project).await?;
    assert!(matches!(
        saved_rejected.decision,
        StoredDecision::Rejected { .. }
    ));
    let absent = fixture
        .store
        .transaction(move |scope| {
            Box::pin(async move {
                let row = query::<sqlx_turso::Turso>(
                    "SELECT count(*) AS count FROM project_directory WHERE project_id=?",
                )
                .bind(rejected_project.as_str())
                .fetch_one(scope.executor())
                .await
                .map_err(StoreError::backend)?;
                row.try_get::<i64, _>("count").map_err(StoreError::backend)
            })
        })
        .await?;
    assert_eq!(
        absent, 0,
        "a refused creation must not synthesize a directory row"
    );
    let before = columns(&fixture).await?;
    assert_eq!(before.len(), 3);
    for row in &before {
        assert!(runtime_primitives::uuid_v7(durable_steps::Uuid::parse_str(
            &row.0
        )?));
        assert!(runtime_primitives::uuid_v7(durable_steps::Uuid::parse_str(
            &row.1
        )?));
        assert_eq!(row.2, SERVICE_A);
        let expected = if row.0 == PROJECT { NOW } else { NOW + 11_000 };
        assert_eq!((row.3, row.4), (expected, expected));
    }
    fixture.io.advance(31 * 24 * 60 * 60 * 1_000);
    assert_eq!(submit(&fixture, &accepted).await?, accepted_answer);
    assert_eq!(submit(&fixture, &rejected).await?, rejected_answer);
    assert_eq!(columns(&fixture).await?, before);
    fixture.close().await?;
    let reopened = Fixture::open(root.path(), NOW + 11_000 + 31 * 24 * 60 * 60 * 1_000).await?;
    assert_eq!(
        binding(&reopened, &saved_accepted.project).await?,
        saved_accepted
    );
    assert_eq!(
        binding(&reopened, &saved_rejected.project).await?,
        saved_rejected
    );
    assert_eq!(columns(&reopened).await?, before);
    reopened.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "The real creation read must refuse corrupt physical timestamps and mismatched ledger carriers."
)]
#[tokio::test]
async fn creation_reads_refuse_fractional_timestamps_and_a_carrier_that_disagrees_with_the_ledger()
-> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    fixture.arrange_registered().await?;
    let project = ProjectId::try_from(PROJECT.to_owned())?;
    let original = binding(&fixture, &project).await?;
    for sql in [
        "UPDATE fleet_creation_decisions SET created_at=1.5",
        "UPDATE fleet_creation_decisions SET updated_at='bad timestamp'",
        "UPDATE fleet_creation_decisions SET carrier_installation='019f0000-0000-7000-8000-000000008102'",
    ] {
        fixture.sql(sql).await?;
        assert!(
            binding(&fixture, &project).await.is_err(),
            "accepted corrupt row: {sql}"
        );
        fixture.sql("UPDATE fleet_creation_decisions SET created_at=1792281600000,updated_at=1792281600000,carrier_installation='019f0000-0000-7000-8000-000000008101'").await?;
        assert_eq!(binding(&fixture, &project).await?, original);
    }
    fixture.close().await
}
