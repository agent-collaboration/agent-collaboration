use super::*;
use crate::subscription_delivery::subscription_service::{
    OwnerObservation, SubscriptionDeliveryServiceProps,
};
use collaboration_contracts::MachineIdentity;
use collaboration_protocol::{EndpointId, EndpointRef, SessionId, UuidIdentity};
use std::sync::Arc;
use tokio::sync::mpsc;

#[tokio::test]
async fn accepted_mail_check_dropped_by_an_idle_owner_is_acknowledged_by_its_replacement()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let directory = tempfile::tempdir()?;
    let clock = Arc::new(router_clock::ManualClock::new(
        "2026-10-19T18:00:00Z".parse()?,
    ));
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
        push_store: interactions,
        delivery: routes.clone(),
        presence: routes,
        machine_identity: MachineIdentity::new(service_id, Some("closing-owner-proof"))?,
        clock: clock.clone(),
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
    let reconcile =
        tokio::spawn(async move { reconciler.observe_native_mail_change(&target).await });
    let accepted = (stale_receiver.recv())
        .await
        .ok_or("stale owner received no command")?;
    let ReaderDeliveryCommand::NativeMailChanged { observed } = accepted else {
        return Err("expected a native mail change command".into());
    };
    stale_receiver.close();
    drop(observed);
    drop(stale_receiver);
    (reconcile).await??;
    let started = (observations.recv()).await?;
    if !matches!(started, OwnerObservation::Started) {
        return Err("replacement owner did not start".into());
    }
    service.shutdown().await;
    Ok(())
}
