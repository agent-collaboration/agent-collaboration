//! The Registry records caller-owned identity and refuses rebinding before writes.
use super::*;
use serde_json::{Value, json};

async fn stored_bindings(fixture: &Fixture) -> TestResult<Value> {
    Ok(fixture.object.transaction(|scope| Box::pin(async move {
        let rows = query::<sqlx_turso::Turso>("SELECT peer_id, service_id, machine_label, access_client_id, credential_hash, state, expires_at, last_enrollment_operation, last_revocation_operation FROM machine_enrollments ORDER BY service_id")
            .fetch_all(scope.executor()).await.map_err(StoreError::backend)?;
        let rows = rows.into_iter().map(|row| {
            let fields = ["peer_id", "service_id", "machine_label", "access_client_id", "credential_hash", "state", "expires_at"]
                .into_iter().map(|field| row.try_get::<String, _>(field).map_err(StoreError::backend))
                .collect::<Result<Vec<_>, _>>()?;
            let operations = ["last_enrollment_operation", "last_revocation_operation"]
                .into_iter().map(|field| row.try_get::<Option<String>, _>(field).map_err(StoreError::backend))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(json!({"fields":fields,"operations":operations}))
        }).collect::<Result<Vec<_>, StoreError>>()?;
        Ok(json!(rows))
    })).await?)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn another_service_cannot_claim_a_recorded_peer_even_after_registry_reopen() -> TestResult {
    // Arrange: the caller supplies its stable identity before enrollment.
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let mut original = request(
        "550e8400-e29b-41d4-a716-4466554400f1",
        "peer-first",
        "peer-first-hash",
    )?;
    original.peer = PeerId::try_from("019f0000-0000-7000-8000-000000000f11".to_owned())?;
    assert_eq!(
        peer(
            fixture
                .runtime
                .enroll_machine(&owner()?, original.clone())
                .await?,
            &original
        )?,
        original.peer
    );
    let before = stored_bindings(&fixture).await?;
    fixture.objects.close().await?;
    drop(fixture);
    let fixture = Fixture::open(root.path()).await?;
    let mut competing = request(
        "550e8400-e29b-41d4-a716-4466554400f2",
        "peer-second",
        "peer-second-hash",
    )?;
    competing.peer = original.peer;

    // Act: no client ID or credential collision hides the peer collision.
    let answer = fixture
        .runtime
        .enroll_machine(&owner()?, competing.clone())
        .await?;

    // Assert: no new row or operation replaces the recorded binding.
    assert_eq!(
        answer,
        EnrollmentDecision::Refused(EnrollmentRefusal::PeerInUse)
    );
    assert_eq!(stored_bindings(&fixture).await?, before);
    assert_eq!(
        fixture.authenticate(&competing).await?,
        Err(AuthRefusal::Unknown)
    );
    assert_eq!(
        fixture
            .authenticate(&original)
            .await?
            .map_err(|r| format!("{r:?}"))?
            .peer(),
        original.peer
    );
    fixture.objects.close().await?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn an_existing_service_cannot_change_its_peer_while_active_revoked_or_readmitting()
-> TestResult {
    // Arrange: real persistent Registry, with only the lifecycle precondition seeded below.
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let original = request(
        "550e8400-e29b-41d4-a716-4466554400f3",
        "stable-peer",
        "stable-peer-hash",
    )?;
    assert_eq!(
        peer(
            fixture
                .runtime
                .enroll_machine(&owner()?, original.clone())
                .await?,
            &original
        )?,
        original.peer
    );
    let mut changed = original.clone();
    changed.peer = PeerId::try_from("019f0000-0000-7000-8000-000000000f13".to_owned())?;
    changed.label = MachineLabel::try_from("must not replace".to_owned())?;
    changed.credential_hash = "must-not-replace-hash".into();
    let active = stored_bindings(&fixture).await?;

    // Act / Assert: an active update refuses the new identity without a write.
    assert_eq!(
        fixture
            .runtime
            .enroll_machine(&owner()?, changed.clone())
            .await?,
        EnrollmentDecision::Refused(EnrollmentRefusal::PeerMismatch)
    );
    assert_eq!(stored_bindings(&fixture).await?, active);

    // The fixture cannot deliver mailbox Open; this seeds revoked state and then
    // exercises actual readmission admission, preserving its pending operation.
    fixture
        .execute("UPDATE machine_enrollments SET state='revoked'")
        .await?;
    let revoked = stored_bindings(&fixture).await?;
    assert_eq!(
        fixture
            .runtime
            .enroll_machine(&owner()?, changed.clone())
            .await?,
        EnrollmentDecision::Refused(EnrollmentRefusal::PeerMismatch)
    );
    assert_eq!(stored_bindings(&fixture).await?, revoked);
    assert_eq!(
        fixture
            .runtime
            .enroll_machine(&owner()?, original.clone())
            .await?,
        EnrollmentDecision::EnrollmentInProgress
    );
    let pending = stored_bindings(&fixture).await?;
    assert_eq!(pending.pointer("/0/fields/0"), Some(&json!(original.peer)));
    assert!(
        pending
            .pointer("/0/operations/0")
            .is_some_and(Value::is_string)
    );
    fixture.objects.close().await?;
    drop(fixture);
    let fixture = Fixture::open(root.path()).await?;
    assert_eq!(
        fixture.runtime.enroll_machine(&owner()?, changed).await?,
        EnrollmentDecision::Refused(EnrollmentRefusal::PeerMismatch)
    );
    assert_eq!(stored_bindings(&fixture).await?, pending);
    assert_eq!(
        fixture.runtime.enroll_machine(&owner()?, original).await?,
        EnrollmentDecision::EnrollmentInProgress
    );
    assert_eq!(stored_bindings(&fixture).await?, pending);
    fixture.objects.close().await?;
    Ok(())
}
