//! First creation decisions outlive feed retention in the real Fleet store.
use super::{
    metadata::request,
    support::{Fixture, TestResult},
};
use durable_steps::Uuid;
use fleet_admission::{FleetOperation, FleetRequestResult, ProjectCreationDecision};
use message_board::{
    BoardId, BoardKind, Description, ProjectId, Refusal, RequestDecision, ResourceName,
    StoredDecision,
};
use runtime_primitives::SyncTime;
use serde_json::json;

async fn binding(fixture: &Fixture, project: &ProjectId) -> TestResult<ProjectCreationDecision> {
    // This constructs only the typed Registry read envelope. It does not stand
    // in for registration admission or claim a live enrolled Registry caller.
    let bytes = fixture
        .runtime
        .on_call(serde_json::to_vec(&json!({
            "source":{"kind":"registry"},"sourceFace":"registry","authority":{"kind":"object"},
            "destination":{"kind":"fleet"},"destinationFace":"fleet",
            "identity":{"operation":Uuid::now_v7(),"step":"creation_read"},
            "kind":"central.creation-decision","target":null,"request":project
        }))?)
        .await
        .map_err(|error| format!("creation leaf did not answer: {error:?}"))?;
    serde_json::from_slice::<Option<ProjectCreationDecision>>(&bytes)?
        .ok_or_else(|| "first creation binding is missing".into())
}

#[tokio::test]
async fn accepted_and_rejected_creation_bindings_survive_terminal_retries_feed_expiry_and_reopen()
-> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let board = BoardId::try_from(Uuid::now_v7().to_string())?;
    let accepted = ProjectId::try_from(Uuid::now_v7().to_string())?;
    let rejected = ProjectId::try_from(Uuid::now_v7().to_string())?;
    let missing_board = BoardId::try_from(Uuid::now_v7().to_string())?;
    let first_accepted = request(FleetOperation::ProjectDirectoryCreate {
        project: accepted.clone(),
        board: board.clone(),
        title: ResourceName::try_from("first accepted creation".to_owned())?,
    })?;
    let first_rejected = request(FleetOperation::ProjectDirectoryCreate {
        project: rejected.clone(),
        board: missing_board.clone(),
        title: ResourceName::try_from("first rejected creation".to_owned())?,
    })?;
    let proof = async {
        for operation in [
            FleetOperation::WorkspaceCreate {name: ResourceName::try_from("creation retention workspace".to_owned())?, description:Description::try_from("real Fleet decisions".to_owned())?},
            FleetOperation::BoardCreate {board:board.clone(),name:ResourceName::try_from("creation retention board".to_owned())?,description:Description::try_from("real parent".to_owned())?,board_kind:BoardKind::Area {}},
        ] {
            if !matches!(fixture.fleet_request(&request(operation)?).await?,FleetRequestResult::Decided(RequestDecision::Accepted {..})) {
                return Err("creation retention parents were not accepted".into());
            }
        }
        let accepted_answer = fixture.fleet_request(&first_accepted).await?;
        if !matches!(accepted_answer,FleetRequestResult::Decided(RequestDecision::Accepted {..})) {
            return Err("first valid creation was not accepted".into());
        }
        let rejected_answer = fixture.fleet_request(&first_rejected).await?;
        if !matches!(&rejected_answer,FleetRequestResult::Decided(RequestDecision::Rejected {reasons}) if reasons == &vec![Refusal::NotFound(missing_board.clone().into())]) {
            return Err("first invalid creation lost its original typed refusal".into());
        }
        let mut expected = Vec::new();
        for (first, original) in [(&first_accepted,&accepted_answer),(&first_rejected,&rejected_answer)] {
            let FleetOperation::ProjectDirectoryCreate {project,..}=&first.operation else {return Err("creation request shape changed".into());};
            let before=binding(&fixture,project).await?;
            if before.project != *project || before.creating_request != first.request_id || before.carrier_installation.as_str() != "550e8400-e29b-41d4-a716-4466554400a9" {
                return Err("creation binding lost the first identity or verified carrier".into());
            }
            if (project==&accepted && !matches!(before.decision,StoredDecision::Accepted {..})) || (project==&rejected && !matches!(&before.decision,StoredDecision::Rejected {reasons} if reasons==&vec![Refusal::NotFound(missing_board.clone().into())])) {
                return Err("creation leaf disagrees with the immutable first ledger".into());
            }
            let another=request(FleetOperation::ProjectDirectoryCreate {project:project.clone(),board:board.clone(),title:ResourceName::try_from("a new request must not revive this ID".to_owned())?})?;
            if !matches!(fixture.fleet_request(&another).await?,FleetRequestResult::Decided(RequestDecision::Rejected {reasons}) if reasons==vec![Refusal::AlreadyExists(project.clone().into())]) {
                return Err("another creation request reused a terminal project ID".into());
            }
            if binding(&fixture,project).await? != before || fixture.fleet_request(first).await? != *original {
                return Err("a fresh request replaced the original binding or replay".into());
            }
            expected.push(before);
        }
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>((expected,accepted_answer,rejected_answer))
    }.await;
    fixture.objects.close().await?;
    let (expected, accepted_answer, rejected_answer) = proof?;
    drop(fixture);

    let reopened = Fixture::open_at(
        root.path(),
        SyncTime::from_unix_millis(1_792_281_600_000 + 31 * 24 * 60 * 60 * 1000),
    )
    .await?;
    let proof=async {
        let update=request(FleetOperation::WorkspaceUpdate {name:ResourceName::try_from("after real retention expiry".to_owned())?,description:Description::try_from("trigger production feed pruning".to_owned())?})?;
        reopened.fleet_request(&update).await?;
        let bytes=reopened.runtime.on_call(serde_json::to_vec(&json!({
            "source":{"kind":"connection","value":"550e8400-e29b-41d4-a716-4466554400a9"},"sourceFace":"session","authority":{"kind":"object"},
            "destination":{"kind":"fleet"},"destinationFace":"fleet",
            "identity":{"operation":Uuid::now_v7(),"step":"expired_feed"},
            "kind":"central.fleet-read","target":null,"request":{"kind":"feed","value":{"after":0}}
        }))?).await.map_err(|error|format!("expired feed did not answer: {error:?}"))?;
        if !matches!(serde_json::from_slice::<board_sync_protocol::BoardReadPage>(&bytes)?,board_sync_protocol::BoardReadPage::FleetFeed(board_sync_protocol::FleetFeedRead::CursorExpired {..})) {
            return Err("retention proof never crossed the real feed expiry".into());
        }
        for original in expected {
            if binding(&reopened,&original.project).await? != original {
                return Err("feed expiry or reopen changed the first creation binding".into());
            }
            let retry=request(FleetOperation::ProjectDirectoryCreate {project:original.project.clone(),board:board.clone(),title:ResourceName::try_from("terminal after expiry".to_owned())?})?;
            if !matches!(reopened.fleet_request(&retry).await?,FleetRequestResult::Decided(RequestDecision::Rejected {reasons}) if reasons==vec![Refusal::AlreadyExists(original.project.clone().into())])
                || binding(&reopened,&original.project).await? != original {
                return Err("feed expiry let a fresh request revive or replace the original creation".into());
            }
        }
        if reopened.fleet_request(&first_accepted).await? != accepted_answer || reopened.fleet_request(&first_rejected).await? != rejected_answer {
            return Err("feed expiry removed an original creation decision".into());
        }
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    reopened.objects.close().await?;
    proof
}
