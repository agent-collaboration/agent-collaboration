use collaboration_service::FleetClient;
use fleet_admission::{FleetIds, FleetOperation, FleetRequest};
use message_board::*;

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove the test contract; Result propagates fixture errors.
async fn stand_in_reply_cache_and_pending_outbox_survive_restart()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let directory = tempfile::tempdir()?;
    let request = FleetRequest {
        request_id: RequestId::generate(),
        workspace: WorkspaceId::generate(),
        actor: RecordedActor {
            actor: message_board::ActorKind::Declared(Identity::Human {
                human_id: HumanId::try_from("caller".to_owned())?,
            }),
            installation: ServiceId::try_from("550e8400-e29b-41d4-a716-446655440000".to_owned())?,
            machine: MachineLabel::try_from("test-machine".to_owned())?,
        },
        acting_for: None,
        resubmits: None,
        protocol_version: ProtocolVersion::CURRENT,
        operation: FleetOperation::WorkspaceCreate {
            name: ResourceName::try_from("work".to_owned())?,
            description: Description::try_from("fleet proof".to_owned())?,
        },
    };
    let client = FleetClient::open_local_stand_in(directory.path()).await?;
    let first = client
        .submit(
            &request,
            &FleetIds {
                generated_record: RecordId::generate(),
            },
        )
        .await?;
    assert!(matches!(
        first.result,
        RequestResult::Decided(RequestDecision::Accepted { .. })
    ));
    assert_eq!(
        client.record(&request.workspace.clone().into()).await?,
        first.records.first().cloned()
    );
    let pending = client.pending_requests().await?;
    assert_eq!(
        pending.len(),
        1,
        "stand-in acceptance does not remove the central outbox request"
    );
    let original_ids = pending[0].ids.clone();
    assert_eq!(pending[0].request, request);
    drop(client);
    let reopened = FleetClient::open_local_stand_in(directory.path()).await?;
    let replay = reopened
        .submit(
            &request,
            &FleetIds {
                generated_record: RecordId::generate(),
            },
        )
        .await?;
    assert_eq!(replay, first);
    assert_eq!(reopened.pending_requests().await?[0].ids, original_ids);
    let mut conflict = request.clone();
    conflict.operation = FleetOperation::WorkspaceCreate {
        name: ResourceName::try_from("different".to_owned())?,
        description: Description::try_from("fleet proof".to_owned())?,
    };
    assert_eq!(
        reopened.submit(&conflict, &original_ids).await?.result,
        RequestResult::ConflictingRequest
    );
    Ok(())
}
