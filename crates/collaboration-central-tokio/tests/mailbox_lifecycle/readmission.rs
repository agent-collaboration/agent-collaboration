//! Actual mailbox calls keep earlier opens and closes harmless across two cycles.
use super::*;

fn identity(step: &str) -> TestResult<CallId> {
    Ok(CallId {
        operation: OperationId::new(Uuid::now_v7())?,
        step: StepName::new(step)?,
    })
}

fn open_call(service: &ServiceId, peer: PeerId, call: &CallId) -> TestResult<Vec<u8>> {
    let mut envelope: Value = serde_json::from_slice(&close_call(service, peer, call)?)?;
    *envelope
        .get_mut("kind")
        .ok_or("lifecycle envelope kind missing")? = json!("central.open-peer-mailbox");
    Ok(serde_json::to_vec(&envelope)?)
}

async fn saved(runtime: &MailRuntime, bytes: &[u8]) -> TestResult {
    assert_eq!(
        runtime
            .on_call(bytes.to_vec())
            .await
            .map_err(|error| format!("saved lifecycle call: {error:?}"))?,
        b"null",
    );
    Ok(())
}

async fn complete(runtime: &MailRuntime, bytes: &[u8]) -> TestResult {
    assert!(
        matches!(
            runtime.on_call(bytes.to_vec()).await,
            Err(CallFailure::AnswerLost)
        ),
        "a newly admitted lifecycle call must not acknowledge before its saved completion",
    );
    runtime.on_alarm().await?;
    saved(runtime, bytes).await
}

#[tokio::test]
async fn two_reopen_cycles_preserve_refusals_and_old_lifecycle_replays_after_store_restart()
-> TestResult {
    // Arrange: literal IDs identify one physical recipient independently of the peer.
    let root = tempfile::tempdir()?;
    let service = ServiceId::try_from("550e8400-e29b-41d4-a716-4466554400ea".to_owned())?;
    let peer = PeerId::new(Uuid::now_v7())?;
    let first_item = ItemId::new(Uuid::now_v7())?;
    let second_item = ItemId::new(Uuid::now_v7())?;
    let close_first = close_call(&service, peer, &identity("close_mailbox")?)?;
    let open_first = open_call(&service, peer, &identity("open_mailbox")?)?;
    let close_second = close_call(&service, peer, &identity("close_mailbox")?)?;
    let open_second = open_call(&service, peer, &identity("open_mailbox")?)?;
    let first = Fixture::open(root.path(), &service).await?;
    assert_eq!(
        first.submit(peer, &service, first_item).await?["kind"],
        "accepted"
    );
    complete(&first.runtime, &close_first).await?;
    let refused_first = first.submit(peer, &service, first_item).await?;
    assert_eq!(refused_first["value"]["status"], revoked());

    // Act: admitting open does not expose new mail before its actual local step.
    assert!(matches!(
        first.runtime.on_call(open_first.clone()).await,
        Err(CallFailure::AnswerLost)
    ));
    assert_eq!(first.submit(peer, &service, second_item).await?, revoked());
    first.runtime.on_alarm().await?;
    saved(&first.runtime, &open_first).await?;

    // Assert: new mail resumes, but open cannot revive an item close refused.
    assert_eq!(
        first.submit(peer, &service, first_item).await?,
        refused_first
    );
    assert_eq!(
        first.submit(peer, &service, second_item).await?["kind"],
        "accepted"
    );
    complete(&first.runtime, &close_second).await?;
    let refused_second = first.submit(peer, &service, second_item).await?;
    assert_eq!(refused_second["value"]["status"], revoked());
    saved(&first.runtime, &open_first).await?;
    assert!(first.runtime.on_alarm().await?.completed.is_empty());
    assert_eq!(
        first
            .submit(peer, &service, ItemId::new(Uuid::now_v7())?)
            .await?,
        revoked()
    );
    first.store.close().await?;
    drop(first);

    let reopened = Fixture::open(root.path(), &service).await?;
    saved(&reopened.runtime, &open_first).await?;
    assert_eq!(
        reopened
            .submit(peer, &service, ItemId::new(Uuid::now_v7())?)
            .await?,
        revoked()
    );
    complete(&reopened.runtime, &open_second).await?;
    // Saved old closes must not close the second readmission, even after reopening.
    saved(&reopened.runtime, &close_first).await?;
    saved(&reopened.runtime, &close_second).await?;
    assert!(reopened.runtime.on_alarm().await?.completed.is_empty());
    assert_eq!(
        reopened
            .submit(peer, &service, ItemId::new(Uuid::now_v7())?)
            .await?["kind"],
        "accepted"
    );
    assert_eq!(
        reopened.submit(peer, &service, first_item).await?,
        refused_first
    );
    assert_eq!(
        reopened.submit(peer, &service, second_item).await?,
        refused_second
    );
    reopened.store.close().await?;
    Ok(())
}
