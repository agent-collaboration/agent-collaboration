use super::*;
use collaboration_protocol::{EndpointId, EndpointRef, SessionId, UuidIdentity};

#[tokio::test]
async fn accepted_reconcile_dropped_by_an_idle_owner_is_acknowledged_by_its_replacement()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let directory = tempfile::tempdir()?;
    let registry = crate::test_project_runtime::registry(directory.path())?;
    let service_id = UuidIdentity::try_from("00000000-0000-4000-8000-000000000001".to_owned())?;
    let target = SessionRef {
        endpoint: EndpointRef {
            service_id: service_id.clone(),
            endpoint_id: EndpointId::try_from("codex-local".to_owned())?,
        },
        session_id: SessionId::try_from("closing-reader".to_owned())?,
    };
    let reader = reader_for_target(&target)?;
    let routes = Arc::new(collaboration_runtime::SessionDeliveryRouter::new(Vec::new()));
    let interactions = Arc::new(
        interaction_storage::InteractionStore::open(directory.path().join("interaction.sqlite"))
            .await?,
    );
    let service = SubscriptionDeliveryService::new(SubscriptionDeliveryServiceProps {
        board_availability: BoardAvailability::Available(Arc::new(Mutex::new(
            super::super::SubscriptionWindowStore::new(registry, interactions.clone()),
        ))),
        push_store: interactions,
        delivery: routes.clone(),
        presence: routes,
        machine_identity: MachineIdentity::new(service_id, Some("closing-owner-proof"))?,
        clock: Arc::new(super::super::SystemSubscriptionClock),
    });
    service.start().await?;
    let mut observations = service.observe_owners();
    // The existing command channel is the race seam: acceptance precedes the
    // stale owner's close, while the response is never acknowledged.
    let (stale_sender, mut stale_receiver) = mpsc::channel(1);
    service
        .inner
        .owners
        .lock()
        .await
        .insert(reader.clone(), stale_sender);
    let reconciler = service.clone();
    let reconcile = tokio::spawn(async move { reconciler.reconcile_reader(reader).await });
    let accepted = tokio::time::timeout(std::time::Duration::from_secs(5), stale_receiver.recv())
        .await?
        .ok_or("stale owner received no command")?;
    let ReaderDeliveryCommand::Reconcile { reply, .. } = accepted else {
        return Err("expected a reconcile command".into());
    };
    stale_receiver.close();
    drop(reply);
    drop(stale_receiver);
    tokio::time::timeout(std::time::Duration::from_secs(5), reconcile).await???;
    let started =
        tokio::time::timeout(std::time::Duration::from_secs(5), observations.recv()).await??;
    if !matches!(started, OwnerObservation::Started) {
        return Err("replacement owner did not start".into());
    }
    service.shutdown().await;
    Ok(())
}
