//! An expiry read cannot turn a real returning native install into quarantine.
use super::*;
use board_central_test_support::{DispatchFaultObservation, ObjectAddress, RuntimeControl};
use board_sync_protocol::ProjectClaimReply;
use fleet_sync_protocol::{DatabaseId, Epoch, PeerId, TransferId};
use received_grants::{wait_published_project, write_and_replicate};

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn membership_expiry_during_a_returning_install_does_not_quarantine_the_project() -> TestResult
{
    let fixture = claim_fixture::ClaimFixture::start().await?;
    let project = ProjectId::generate();
    let (_, expected) = project_claim::imported_snapshot(
        &fixture.central,
        &fixture.directory.path().join("source.sqlite"),
        &project,
    )
    .await?;
    assert!(
        matches!(fixture.claim(&project).await?, ProjectClaimReply::Claimed { epoch, .. } if epoch.value() == 2)
    );
    write_and_replicate(
        &fixture.projects,
        &fixture.registry,
        &project,
        2,
        &expected,
        origin()?,
    )
    .await?;
    let second_origin = ActorOrigin {
        installation: "550e8400-e29b-41d4-a716-446655441087"
            .to_owned()
            .try_into()?,
        machine: "expiry roundtrip receiver".to_owned().try_into()?,
    };
    let second_machine = fixture
        .central
        .additional_machine(&second_origin, PeerId::new(durable_steps::Uuid::now_v7())?)
        .await?;
    let second_board = open_board_admission(
        &fixture.directory.path().join("second-router"),
        MachineIdentity::new(
            UuidIdentity::try_from(second_origin.installation.as_str().to_owned())?,
            Some(second_origin.machine.as_str()),
        )?,
        Some(second_machine.session_inputs()?),
        None,
    )
    .await?;
    let second_projects = second_board
        .session
        .as_ref()
        .ok_or("second session")?
        .projects
        .clone();
    let second_registry = second_projects.registry()?;
    let second_admission = second_board.admission.clone();
    let second_running = second_board.start().ok_or("second running session")?;
    wait_welcome(second_projects.welcomed.clone()).await?;
    let connection = ObjectAddress::Connection(origin()?.installation);
    let proof = async {
        let database = DatabaseId::try_from(project.as_str().to_owned())?;
        let moved = fixture.central.move_project(database, &second_origin.installation, TransferId::new(durable_steps::Uuid::now_v7())?).await?;
        assert_eq!(moved.pointer("/value/newEpoch"), Some(&serde_json::json!(3)), "{moved}");
        let moved_store = wait_published_project(&second_registry, &project, 3).await?;
        drop(moved_store);
        write_and_replicate(&second_projects, &second_registry, &project, 3, &expected, second_origin.clone()).await?;
        timeout(BOUND, async {
            while fixture.registry.superseded_current(&project).await.is_none() {
                tokio::task::yield_now().await;
            }
        }).await?;
        let old_path = fixture.projects.directory.join("projects").join(project.as_str()).join("e2.sqlite");
        let old_bytes = durable_file_hashes(&old_path)?;
        let mut notice = fixture.central.hold_granted_notice(project.clone(), Epoch::new(4)?)?;
        let returned = fixture.central.take_over_project(database, &origin()?.installation, TransferId::new(durable_steps::Uuid::now_v7())?).await?;
        assert_eq!(returned.pointer("/value/newEpoch"), Some(&serde_json::json!(4)), "{returned}");
        timeout(BOUND, notice.wait_held()).await??;
        let grant = fixture.projects.confirmed_snapshot_grant(&project, Epoch::new(4)?).await?.ok_or("real return grant")?;
        let start = grant.start;
        fixture.central.runtime_control(connection.clone(), RuntimeControl::HoldNextDispatch {
            destination: ObjectAddress::Copy(database),
        }).await?;
        fixture.projects.prepare_snapshot_install(&project, grant).await?;
        let observed = timeout(BOUND, fixture.central.runtime_control(connection.clone(), RuntimeControl::AwaitHeldDispatch)).await??;
        assert!(matches!(observed.dispatch, DispatchFaultObservation::Held { destination: ObjectAddress::Copy(actual), .. } if actual == database));
        assert!(fixture.registry.existing_project(&project).await.is_err(), "unverified return seed became writable");

        // The same read called by ReaderDeliveryOwner's expiry pass runs while
        // the actual receiver cannot finish. No fabricated registry error.
        let interactions = Arc::new(interaction_storage::InteractionStore::open(
            fixture.directory.path().join("expiry-interactions.sqlite"),
        ).await?);
        let windows = collaboration_service::SubscriptionWindowStore::new(fixture.registry.clone(), interactions.clone());
        let reader = Identity::Human { human_id: "expiry scan reader".to_owned().try_into()? };
        let expiry = windows.scan_membership_expiry_for_test(&reader, std::slice::from_ref(&project)).await;
        let registry_state = fixture.registry.is_superseded(&project).await;
        drop(windows);
        Arc::try_unwrap(interactions).map_err(|_| "expiry store retained")?.close().await?;
        if let Err(error) = registry_state {
            return Err(format!("expiry scan quarantined the real receiving install: {error}").into());
        }
        assert!(expiry.is_err(), "installing project must remain retryable, not be discarded as definitive");
        fixture.central.runtime_control(connection.clone(), RuntimeControl::ResumeDispatch).await?;
        notice.release()?;
        let store = wait_published_project(&fixture.registry, &project, 4).await?;
        let mut read = store.snapshot().await?;
        let actual = read.record(&project.clone().into()).await?.ok_or("returned prefix")?;
        read.finish().await?;
        assert!(matches!(&actual, Record::Project(state) if state.description.as_str() == "written by live epoch 3"));
        let native = fixture.registry.database(&project).await?;
        let receipt = native.installed_snapshot().await?.ok_or("returning install receipt")?;
        assert!(receipt.ready && receipt.verified);
        assert_eq!(receipt.install.position, start);
        assert_eq!(native.path().file_name().and_then(|name| name.to_str()), Some("e4.sqlite"));
        drop(native);
        drop(store);
        assert_eq!(durable_file_hashes(&old_path)?, old_bytes);
        write_and_replicate(&fixture.projects, &fixture.registry, &project, 4, &actual, origin()?).await?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    let _ = fixture
        .central
        .runtime_control(connection, RuntimeControl::ResumeDispatch)
        .await;
    let stopped = second_running.shutdown().await;
    drop(second_projects);
    drop(second_registry);
    let closed = Arc::try_unwrap(second_admission)
        .map_err(|_| "second admission retained")?
        .close()
        .await;
    let second_edge = second_machine.shutdown().await;
    let first_closed = fixture.shutdown().await;
    proof?;
    stopped?;
    closed?;
    second_edge?;
    first_closed?;
    Ok(())
}
