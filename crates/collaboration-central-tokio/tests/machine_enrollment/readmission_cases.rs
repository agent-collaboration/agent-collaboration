//! Pending readmission preserves the Registry row and its unique identities.
use super::*;
use durable_steps::{OperationId, OperationState, storage::JournalStorage};
use serde_json::json;

struct ActivationWindow {
    _root: tempfile::TempDir,
    fixture: Fixture,
    enrollment: EnrollMachine,
    peer: PeerId,
}
async fn activation_window() -> TestResult<ActivationWindow> {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let enrollment = request(
        "550e8400-e29b-41d4-a716-4466554400e8",
        "window-client",
        "window-hash",
    )?;
    let stable = peer(
        fixture
            .runtime
            .enroll_machine(&owner()?, enrollment.clone())
            .await?,
        &enrollment,
    )?;
    fixture
        .execute("UPDATE machine_enrollments SET state='revoked'")
        .await?;
    assert_eq!(
        fixture
            .runtime
            .enroll_machine(&owner()?, enrollment.clone())
            .await?,
        EnrollmentDecision::EnrollmentInProgress
    );
    // This seeds the persisted activation-before-finish precondition. The real
    // open and a held engine finish remain separate C20 integration obligations.
    fixture
        .execute("UPDATE machine_enrollments SET state='enrolled'")
        .await?;
    let before = pending_snapshot(&fixture).await?;
    assert_eq!(
        before
            .get("fields")
            .and_then(|fields| fields.get(1))
            .and_then(serde_json::Value::as_str),
        Some("enrolled"),
    );
    assert_eq!(
        fixture
            .authenticate(&enrollment)
            .await?
            .map_err(|error| format!("{error:?}"))?
            .peer(),
        stable
    );
    Ok(ActivationWindow {
        _root: root,
        fixture,
        enrollment,
        peer: stable,
    })
}

#[tokio::test]
async fn an_active_same_content_repeat_answers_enrolled_while_engine_finish_is_pending()
-> TestResult {
    let window = activation_window().await?;
    let before = pending_snapshot(&window.fixture).await?;
    assert_eq!(
        peer(
            window
                .fixture
                .runtime
                .enroll_machine(&owner()?, window.enrollment.clone())
                .await?,
            &window.enrollment
        )?,
        window.peer
    );
    assert_eq!(pending_snapshot(&window.fixture).await?, before);
    let mut overlap = window.enrollment;
    overlap.label = MachineLabel::try_from("must remain pending".to_owned())?;
    assert_eq!(
        window
            .fixture
            .runtime
            .enroll_machine(&owner()?, overlap)
            .await?,
        EnrollmentDecision::EnrollmentInProgress
    );
    assert_eq!(pending_snapshot(&window.fixture).await?, before);
    window.fixture.objects.close().await?;
    Ok(())
}

#[tokio::test]
async fn an_active_row_revokes_immediately_while_its_enrollment_finish_is_pending() -> TestResult {
    let window = activation_window().await?;
    let before = pending_snapshot(&window.fixture).await?;
    assert_eq!(
        window
            .fixture
            .runtime
            .revoke_machine(&owner()?, window.enrollment.installation.clone())
            .await?,
        collaboration_central_core::RevocationDecision::MachineRevoked
    );
    assert_eq!(
        window.fixture.authenticate(&window.enrollment).await?,
        Err(AuthRefusal::Revoked)
    );
    let after = pending_snapshot(&window.fixture).await?;
    assert_eq!(after["operation"], before["operation"]);
    assert_eq!(after["fields"][1], "revoked");
    assert!(after["revoke"].is_string());
    window.fixture.objects.close().await?;
    Ok(())
}

async fn pending_snapshot(fixture: &Fixture) -> TestResult<serde_json::Value> {
    Ok(fixture.object.transaction(|scope| Box::pin(async move {
        let row = query::<sqlx_turso::Turso>("SELECT peer_id, state, machine_label, access_client_id, credential_hash, expires_at, last_enrollment_operation, last_revocation_operation FROM machine_enrollments")
            .fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
        let operation: Option<String> = row.try_get("last_enrollment_operation").map_err(StoreError::backend)?;
        let operation = operation.ok_or_else(|| StoreError::Aborted { reason: "readmission was not journaled".into() })?;
        let record = scope.operation(OperationId::try_from(operation).map_err(StoreError::backend)?)
            .await?.ok_or_else(|| StoreError::Aborted { reason: "readmission journal missing".into() })?;
        assert_eq!(record.kind, "central.readmit-machine");
        assert!(matches!(record.status().state, OperationState::Running { .. }));
        let fields = ["peer_id", "state", "machine_label", "access_client_id", "credential_hash", "expires_at"]
            .into_iter().map(|field| row.try_get::<String, _>(field).map_err(StoreError::backend))
            .collect::<Result<Vec<_>, _>>()?;
        let revoke: Option<String> = row.try_get("last_revocation_operation").map_err(StoreError::backend)?;
        Ok(json!({"fields":fields,"operation":record.id,"revoke":revoke}))
    })).await?)
}

#[tokio::test]
async fn an_unanswered_open_keeps_reserved_readmission_revoked_and_refuses_overlapping_changes()
-> TestResult {
    use collaboration_central_core::RevocationDecision;

    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let owner = owner()?;
    let original = request(
        "550e8400-e29b-41d4-a716-4466554400ed",
        "old-client",
        "old-hash",
    )?;
    let stable = peer(
        fixture
            .runtime
            .enroll_machine(&owner, original.clone())
            .await?,
        &original,
    )?;
    // Seed the precondition already used by the identity/reopen case. The
    // Registry journal and credential writes below are real; completed revoke,
    // mailbox open and alarm wiring are separate integration obligations.
    fixture
        .execute("UPDATE machine_enrollments SET state='revoked'")
        .await?;
    let mut reserved = original.clone();
    reserved.access_client_id = AccessClientId::new("reserved-client")?;
    reserved.credential_hash = "reserved-hash".into();
    reserved.label = MachineLabel::try_from("reserved readmission".to_owned())?;
    assert_eq!(
        fixture
            .runtime
            .enroll_machine(&owner, reserved.clone())
            .await?,
        EnrollmentDecision::EnrollmentInProgress
    );
    assert_eq!(
        fixture.authenticate(&reserved).await?,
        Err(AuthRefusal::Revoked)
    );
    assert_eq!(
        fixture.authenticate(&original).await?,
        Err(AuthRefusal::Unknown)
    );
    let before = pending_snapshot(&fixture).await?;
    assert_eq!(before["fields"][0], stable.to_string());
    assert_eq!(before["fields"][1], "revoked");
    for proposed in [reserved.clone(), {
        let mut other = reserved.clone();
        other.label = MachineLabel::try_from("must not be reserved".to_owned())?;
        other
    }] {
        assert_eq!(
            fixture.runtime.enroll_machine(&owner, proposed).await?,
            EnrollmentDecision::EnrollmentInProgress
        );
        assert_eq!(
            pending_snapshot(&fixture).await?,
            before,
            "pending enrollment was overwritten"
        );
    }
    assert_eq!(
        fixture
            .runtime
            .revoke_machine(&owner, original.installation)
            .await?,
        RevocationDecision::EnrollmentInProgress
    );
    assert_eq!(
        pending_snapshot(&fixture).await?,
        before,
        "already-revoked replay bypassed the enrollment gate"
    );
    let competing = request(
        "550e8400-e29b-41d4-a716-4466554400ee",
        "reserved-client",
        "unused-hash",
    )?;
    assert_eq!(
        fixture.runtime.enroll_machine(&owner, competing).await?,
        EnrollmentDecision::Refused(EnrollmentRefusal::AccessClientInUse)
    );
    assert_eq!(pending_snapshot(&fixture).await?, before);
    fixture.objects.close().await?;
    drop(fixture);
    let fixture = Fixture::open(root.path()).await?;
    assert_eq!(
        fixture.authenticate(&reserved).await?,
        Err(AuthRefusal::Revoked)
    );
    assert_eq!(pending_snapshot(&fixture).await?, before);
    assert_eq!(
        fixture.runtime.enroll_machine(&owner, reserved).await?,
        EnrollmentDecision::EnrollmentInProgress
    );
    assert_eq!(pending_snapshot(&fixture).await?, before);
    fixture.objects.close().await?;
    Ok(())
}
