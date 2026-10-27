//! A real Welcome repairs a missed publication prompt after a durable copy acknowledgement.
use super::*;
use crate::collaboration_runtime::board_composition::tests::notification_publisher::project_fixture::seed_unacknowledged_notification;
use collaboration_service::{CollaborationApplication, MachineIdentity, ProjectAdmissionProps, ServiceIdentity, SessionDeliveryRouter};

#[tokio::test]
async fn welcome_publishes_an_eligible_remote_handoff_after_its_acknowledgement_hint_was_missed()
-> Result<(), TestError> {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::WARN)
        .with_test_writer()
        .try_init();
    let directory = tempfile::tempdir()?;
    let root = directory.path().join("router");
    let mut central = TestFleet::start_mail_as(&directory.path().join("central"), None).await?;
    let seed = seed_unacknowledged_notification(&root, &mut central).await?;
    // The same service, peer, receiver and saved client session survive. Only
    // the original Router's native file handles and loops have closed.
    let mut harness =
        MailHarness::open_configured(directory.path(), None, false, true, None, Some(central))
            .await?;
    let fleet = Arc::new(
        FleetClient::open(
            &harness.root,
            harness.projects.clone(),
            Some(harness.requests.clone()),
        )
        .await?,
    );
    let machine = MachineIdentity::new(
        "00000000-0000-4000-8000-000000000001"
            .to_owned()
            .try_into()?,
        Some("notice fixture"),
    )?;
    let admission = Arc::new(ProjectAdmission::new(ProjectAdmissionProps {
        registry: harness.registry.clone(),
        fleet: fleet.clone(),
        identity: machine,
    })?);
    let replication: Arc<dyn collaboration_service::ProjectReplicationPosition> =
        harness.projects.clone();
    let identity = ServiceIdentity::new(
        "00000000-0000-4000-8000-000000000001",
        "00000000-0000-4000-8000-000000000002",
    )?
    .with_project_admission(admission.clone())
    .with_project_replication(replication.clone())
    .with_automation_store(harness.mail.automation.clone())
    .with_subscription_delivery_service(
        harness.mail.actors.clone(),
        Arc::new(SessionDeliveryRouter::new(Vec::new())),
    )
    .with_interaction_store(harness.mail.owner.clone())
    .with_installation_peer(harness.peer())
    .map_err(io::Error::other)?
    .with_mail_delivery_runtime(harness.mail.mail.clone())
    .map_err(io::Error::other)?;
    let publisher = Arc::new(collaboration_service::NotificationPublisher::new(
        admission.clone(),
        harness.mail.owner.clone(),
        Arc::downgrade(&harness.mail.mail),
    ));
    harness.publisher = Arc::downgrade(&publisher);
    let app =
        CollaborationApplication::new(identity.with_notification_publisher(publisher.clone()));
    harness.admission = Arc::downgrade(&admission);
    harness.fleet = Arc::downgrade(&fleet);
    assert!(!*harness.welcomed.borrow());
    assert!(
        replication
            .acknowledged_mail_changes(&seed.project)
            .await?
            .max_handoff_seq
            .is_none()
    );
    publisher.queue_project(seed.project.clone()).await?;
    tokio::time::timeout(BOUND, async {
        loop {
            if harness
                .mail
                .owner
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
    .map_err(|_| io::Error::other("publisher did not save its unacknowledged scan prefix"))??;
    let writer = harness.projects.ensure(&seed.project).await?;
    assert!(matches!(
        writer.store.writer_state().await?,
        fleet_sync_lease::WriterState::Writing { .. }
    ));
    // Execute the real SDK copy while its real Welcome is gated before the Host.
    // Discard only this returned prompt, modelling a gap in event delivery.
    // No source row, copy cursor, acknowledgement or frame is fabricated.
    tokio::time::timeout(BOUND, async {
        loop {
            let events = writer
                .sender
                .replicate_once(harness.central.receiver)
                .await?;
            assert!(events.iter().any(|event| matches!(
                event,
                fleet_sync_writer::ReplicationEvents::Acknowledged { .. }
            )));
            if replication
                .acknowledged_mail_changes(&seed.project)
                .await?
                .max_handoff_seq
                == Some(1)
            {
                return Ok::<_, TestError>(());
            }
        }
    })
    .await??;
    assert_eq!(
        replication
            .acknowledged_mail_changes(&seed.project)
            .await?
            .max_handoff_seq,
        Some(1)
    );
    assert!(
        !harness
            .central
            .notifications
            .has_submission(seed.item)
            .await
    );
    harness.start_mail();
    harness.release_welcome()?;
    let mut initial = Vec::new();
    loop {
        let event = harness.next_manual_event().await?;
        let welcome = matches!(event, ClientEvent::Welcome { .. });
        initial.push(event);
        if welcome {
            break;
        }
    }
    harness.resume_events(initial).await?;
    let publication = tokio::time::timeout(
        Duration::from_secs(10),
        harness.central.notifications.wait_submission(seed.item),
    )
    .await
    .map_err(|_| io::Error::other("eligible remote handoff stayed pending after its real Welcome"));
    publisher.shutdown().await;
    harness.mail.actors.shutdown().await;
    drop(app);
    drop(publisher);
    drop(writer);
    drop(admission);
    drop(fleet);
    drop(replication);
    harness.close().await?;
    assert_eq!(
        publication?,
        Some(fleet_sync_protocol::Epoch::new(seed.epoch.get())?)
    );
    Ok(())
}
