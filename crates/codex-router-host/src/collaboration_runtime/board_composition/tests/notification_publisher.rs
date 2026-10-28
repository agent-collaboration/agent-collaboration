//! Native sender acknowledgements must prompt publication without another board write.
use super::snapshot_restart::board_fixture::native_mail_owner::NativeMailOwner;
use super::*;
use collaboration_runtime::SessionDeliveryRouter;
use collaboration_service::ApplicationFixture;
use collaboration_service::BoardAvailability;
use collaboration_service::IntentHandoffInput;
use collaboration_service::IntentRecipientEnrollment;
use collaboration_service::MailPushDeliveryProps;
use collaboration_service::SubscriptionDeliveryService;
use collaboration_service::SubscriptionDeliveryServiceProps;
use collaboration_service::SystemSubscriptionClock;
use fleet_sync_protocol::{ItemId, PeerId};
use interaction_storage::InteractionStore;
use tokio::sync::Mutex;
#[path = "notification_project_fixture.rs"]
pub(in crate::collaboration_runtime::board_composition) mod project_fixture;

async fn submit_operation(
    admission: &ProjectAdmission,
    request: &ProjectCreateRequest,
    operation: BoardOperation,
) -> TestResult {
    let result = admission
        .submit(&ProjectRequest {
            request_id: RequestId::generate(),
            project_id: request.project_id.clone(),
            actor: request.actor.clone(),
            acting_for: None,
            protocol_version: ProtocolVersion::CURRENT,
            operation,
        })
        .await?;
    assert!(
        matches!(
            result.result,
            RequestResult::Decided(RequestDecision::Accepted { .. })
        ),
        "{result:?}"
    );
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn an_acknowledged_remote_handoff_is_published_without_another_board_write() -> TestResult {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::WARN)
        .with_test_writer()
        .try_init();
    let directory = tempfile::tempdir()?;
    let root = directory.path().join("router");
    let mut central = TestFleet::start_mail_as(&directory.path().join("central"), None).await?;
    // Seed before composing the publisher. Its earlier real ACK prompts must
    // not race the fixture's explicit claim and select a different item.
    let seed = Box::pin(project_fixture::seed_unacknowledged_notification(
        &root,
        &mut central,
    ))
    .await?;
    let machine = MachineIdentity::new(
        "00000000-0000-4000-8000-000000000001"
            .to_owned()
            .try_into()?,
        Some("notice fixture"),
    )?;
    let mut board = open_board_admission(&root, machine.clone(), Some(central.inputs()?)).await?;
    let projects = board.session.as_ref().ok_or("session")?.projects.clone();
    let welcomed = board
        .session
        .as_ref()
        .ok_or("session")?
        .welcomed
        .subscribe();
    let replication = board
        .replication
        .as_ref()
        .ok_or("replication port")?
        .clone();
    let admission = board.admission.clone();
    let progress = Arc::new(Mutex::new(
        automation_storage::AutomationStore::open(&root.join("notification-progress.sqlite"))
            .await?,
    ));
    let carrier = Arc::new(SessionDeliveryRouter::new(Vec::new()));
    let interactions = Arc::new(InteractionStore::open(root.join("interaction.sqlite")).await?);
    let actors = SubscriptionDeliveryService::new(SubscriptionDeliveryServiceProps {
        board_availability: BoardAvailability::Unavailable,
        push_store: interactions.clone(),
        delivery: carrier.clone(),
        presence: carrier.clone(),
        machine_identity: machine.clone(),
        clock: Arc::new(SystemSubscriptionClock),
    });
    actors.start().await?;

    let identity = ApplicationFixture::new(
        &String::from(machine.service_id().clone()),
        "00000000-0000-4000-8000-000000000002",
    )?
    .with_machine_identity(machine.clone())
    .map_err(io::Error::other)?
    .with_automation_store(progress.clone())
    .with_project_admission(admission.clone())
    .with_project_replication(replication.clone())
    .with_subscription_delivery_service(actors.clone(), carrier.clone());
    let (identity, mail) = NativeMailOwner::start_with_session(
        identity,
        central.peer,
        MailPushDeliveryProps {
            interactions: interactions.clone(),
            actors: actors.clone(),
            presence: carrier,
            machine,
            clock: Arc::new(SystemSubscriptionClock),
        },
        board.mail_session(),
    )?;
    let mail_handle = mail.runtime_handle();
    board.attach_mail(&mail_handle);
    let publisher = Arc::new(collaboration_service::NotificationPublisher::new(
        admission.clone(),
        interactions.clone(),
        Arc::downgrade(&mail_handle),
    ));
    board.attach_publisher(&publisher);
    let app = (identity.with_notification_publisher(publisher.clone())).build();
    let (piece_entered, release_piece) = central.notifications.hold_next_piece().await?;
    let running = board.start().ok_or("session start")?;
    wait_welcome(welcomed).await?;
    timeout(BOUND, piece_entered)
        .await
        .map_err(|_| io::Error::other("real handoff copy did not reach held receiver"))??;
    // Only Welcome has queued this new publisher. Its durable write proves the
    // pass observed Pending while the real handoff piece is still blocked.
    timeout(BOUND, async {
        loop {
            if interactions
                .intents_scanned_through(&seed.project)
                .await?
                .is_some()
            {
                return Ok::<_, interaction_storage::StorageError>(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(|_| io::Error::other("publisher did not save the unacknowledged scan prefix"))??;
    assert!(!central.notifications.has_submission(seed.item).await);
    assert!(
        replication
            .acknowledged_mail_changes(&seed.project)
            .await?
            .max_handoff_seq
            .is_none()
    );
    release_piece
        .send(())
        .map_err(|_| io::Error::other("receiver gate closed"))?;
    timeout(BOUND, async {
        loop {
            projects.replicate_project_for_test(&seed.project).await?;
            if replication
                .acknowledged_mail_changes(&seed.project)
                .await?
                .max_handoff_seq
                == Some(1)
            {
                return TestResult::Ok(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await??;
    // This central fixture refuses Project custody. The observation is the real
    // publication request with its frozen identity and epoch, not G3 authority.
    let publication = timeout(
        std::time::Duration::from_secs(10),
        central.notifications.wait_submission(seed.item),
    )
    .await
    .map_err(|_| {
        io::Error::other("eligible remote handoff stayed pending after its native acknowledgement")
    });
    publisher.shutdown().await;
    actors.shutdown().await;
    drop(app);
    drop(publisher);
    drop(mail_handle);
    running.shutdown().await?;
    drop(actors);
    drop(interactions);
    mail.close().await?;
    drop(progress);
    drop(projects);
    drop(replication);
    Arc::try_unwrap(admission)
        .map_err(|_| io::Error::other("notification admission retained"))?
        .close()
        .await?;
    central.stop().await?;
    assert_eq!(
        publication?,
        Some(fleet_sync_protocol::Epoch::new(seed.epoch.get())?)
    );
    Ok(())
}
