use super::*;
use fixture::{Live, Reader, assert_visible, create};

#[expect(
    clippy::panic_in_result_fn,
    reason = "Behavioral assertions must fail while fixture errors propagate."
)]
#[tokio::test]
async fn decided_write_survives_refused_cache_refresh_and_same_key_replay() -> TestResult {
    let directory = tempfile::tempdir()?;
    let router_root = directory.path().join("router");
    let jwks = JwksFixtureServer::start().await?;
    let live = Live::start(&directory.path().join("central"), &jwks.url).await?;
    let reader = Reader::start(&router_root, live.server.base_url()).await?;
    let offered = create()?;
    let origin = ActorOrigin {
        installation: READER_SERVICE.to_owned().try_into()?,
        machine: "cache-reader".to_owned().try_into()?,
    };
    reader.ingress.controls.refuse_feed_reads();
    let returned = reader.fleet.submit(&offered, &origin).await;
    let outbox =
        message_board_storage::FleetOutbox::open(&router_root.join("fleet-outbox.sqlite")).await?;
    let saved = outbox
        .request(&offered.request_id)
        .await?
        .ok_or("decided outbox entry missing")?;
    let message_board_storage::FleetOutboxState::Decided { decision } = saved.state else {
        return Err("central's accepted write was not durably decided".into());
    };
    assert!(
        matches!(&decision, RequestDecision::Accepted { effects, change: ChangeOutcome::Changed }
        if matches!(effects.entries(), [(reference, RecordPlacement::Fleet(position))]
            if reference == &offered.workspace.clone().into() && position.position.get() == 1 && position.origin == origin))
    );
    assert!(
        returned.is_ok(),
        "durably accepted decision was replaced by refresh failure: {returned:?}"
    );
    let returned = returned?;
    assert_eq!(returned.result, RequestResult::Decided(decision));
    assert!(
        reader.ingress.controls.refused_feed_count() > 0,
        "remote feed refusal was not reached"
    );
    let cache =
        message_board_storage::FleetCache::open(&router_root.join("fleet-cache.sqlite")).await?;
    assert_eq!(cache.checkpoint().await?, FleetPosition::try_from(0)?);
    assert!(
        reader
            .fleet
            .record(&offered.workspace.clone().into())
            .await?
            .is_none()
    );
    assert_eq!(reader.fleet.submit(&offered, &origin).await?, returned);
    assert_eq!(
        reader.ingress.controls.fleet_write_count(),
        1,
        "keyed replay sent another write"
    );
    cache.close().await;
    outbox.close().await;
    reader.shutdown().await?;
    live.shutdown().await?;
    jwks.shutdown().await?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn pending_request_replays_through_the_session_and_keeps_central_provenance_after_reopen()
-> TestResult {
    let directory = tempfile::tempdir()?;
    let server_root = directory.path().join("central");
    let router_root = directory.path().join("router");
    let offered = create()?;
    let forged_origin = ActorOrigin {
        installation: ServiceId::try_from(WRITER_SERVICE.to_owned())?,
        machine: MachineLabel::try_from("caller supplied label must be ignored".to_owned())?,
    };
    let offline = FleetClient::open(&router_root, Arc::new(NoRegistration), None).await?;
    assert_eq!(
        offline.submit(&offered, &forged_origin).await?.result,
        RequestResult::PendingUnreachable
    );
    let pending = offline.pending_requests().await?;
    assert_eq!(pending.len(), 1);
    assert_eq!(
        pending.first().ok_or("missing pending request")?.request,
        offered
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
    let first = reader.fleet.submit(&offered, &forged_origin).await?;
    assert!(
        matches!(&first.result, RequestResult::Decided(RequestDecision::Accepted { effects, .. }) if effects.entries() == [(offered.workspace.clone().into(), workspace.envelope.position)])
    );
    let mut conflict = offered.clone();
    conflict.operation = FleetOperation::WorkspaceCreate {
        name: ResourceName::try_from("different body".to_owned())?,
        description: Description::try_from(String::new())?,
    };
    assert_eq!(
        reader.fleet.submit(&conflict, &forged_origin).await?.result,
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
    assert_eq!(reader.fleet.submit(&offered, &forged_origin).await?, first);
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
