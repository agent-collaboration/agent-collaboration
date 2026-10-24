use super::*;
use fixture::{Live, Reader, assert_visible, create};

#[tokio::test]
async fn pending_request_replays_through_the_session_and_keeps_central_provenance_after_reopen()
-> TestResult {
    let directory = tempfile::tempdir()?;
    let server_root = directory.path().join("central");
    let router_root = directory.path().join("router");
    let offered = create()?;
    let original_ids = FleetIds {
        generated_record: RecordId::generate(),
    };
    let forged_origin = ActorOrigin {
        installation: ServiceId::try_from(WRITER_SERVICE.to_owned())?,
        machine: MachineLabel::try_from("caller supplied label must be ignored".to_owned())?,
    };
    let offline = FleetClient::open(&router_root, Arc::new(NoRegistration), None).await?;
    assert_eq!(
        offline
            .submit(&offered, &original_ids, &forged_origin)
            .await?
            .result,
        RequestResult::PendingUnreachable
    );
    let pending = offline.pending_requests().await?;
    assert_eq!(pending.len(), 1);
    assert_eq!(
        pending.first().ok_or("missing pending request")?.request,
        offered
    );
    assert_eq!(
        pending.first().ok_or("missing pending request")?.ids,
        original_ids
    );
    assert!(!router_root.join("fleet.sqlite").exists());
    offline.close().await;

    let jwks = JwksFixtureServer::start().await?;
    let live = Live::start(&server_root, &jwks.url).await?;
    let reader = Reader::start(&router_root, live.server.base_url()).await?;
    reader.fleet.synchronize().await?;
    assert!(reader.fleet.pending_requests().await?.is_empty());
    assert_visible(&reader.fleet, &router_root, "old visible workspace", 2).await?;
    let Some(Record::Workspace(workspace)) = reader
        .fleet
        .record(&offered.workspace.clone().into())
        .await?
    else {
        return Err("accepted workspace missing from Router cache".into());
    };
    let RecordPlacement::Fleet(placement) = &workspace.envelope.position else {
        return Err("workspace lost its Fleet placement".into());
    };
    assert_eq!(placement.position.get(), 1);
    assert_eq!(placement.origin.installation.as_str(), READER_SERVICE);
    assert_eq!(placement.origin.machine.as_str(), "cache-reader");
    let retry_ids = FleetIds {
        generated_record: RecordId::generate(),
    };
    let first = reader
        .fleet
        .submit(&offered, &retry_ids, &forged_origin)
        .await?;
    assert!(
        matches!(&first.result, RequestResult::Decided(RequestDecision::Accepted { effects, .. }) if effects.entries() == [(offered.workspace.clone().into(), workspace.envelope.position)])
    );
    let mut conflict = offered.clone();
    conflict.operation = FleetOperation::WorkspaceCreate {
        name: ResourceName::try_from("different body".to_owned())?,
        description: Description::try_from(String::new())?,
    };
    assert_eq!(
        reader
            .fleet
            .submit(&conflict, &retry_ids, &forged_origin)
            .await?
            .result,
        RequestResult::ConflictingRequest
    );
    let effects_request = RequestEffectsRequest {
        scope: RequestEffectsScope::Fleet {
            workspace_id: offered.workspace.clone(),
        },
        request: offered.request_id.clone(),
        after: None,
    };
    let receipt = reader.fleet.request_effects(&effects_request).await?;
    reader.shutdown().await?;
    let reader = Reader::start(&router_root, live.server.base_url()).await?;
    reader.fleet.synchronize().await?;
    assert_eq!(
        reader
            .fleet
            .submit(&offered, &retry_ids, &forged_origin)
            .await?,
        first
    );
    assert_eq!(
        reader.fleet.request_effects(&effects_request).await?,
        receipt
    );
    assert!(!router_root.join("fleet.sqlite").exists());
    reader.shutdown().await?;
    live.shutdown().await?;
    jwks.shutdown().await?;
    Ok(())
}
