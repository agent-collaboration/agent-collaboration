use super::{
    central_pair::{self, SecondHost},
    fixture::{BoardFixture, identity},
};
use board_central_test_support::MailWireEvent;
use codex_router_test_support::{
    FixtureResult, owned_peer_fixture::PeerFixture, scenario_finalization::run_scenario,
};
use collaboration_client::protocol::{EndpointRef, SessionRef};
use fleet_sync_protocol::{DeliveryStatus, ItemId};
use serde_json::{Value, json};
use std::time::Duration;

pub async fn lost_live_callback(
    fixture: &mut BoardFixture,
    second: &mut SecondHost,
) -> FixtureResult {
    // The override belongs only to the owned feature-gated test Host.
    fixture.stop_host().await?;
    let item: ItemId = identity().try_into()?;
    let mut wire = central_pair::central(fixture)?.observe_mail_wire(Some(item))?;
    std::fs::write(
        fixture.directory.join("fixture-mail-status-interval-ms"),
        "2000",
    )?;
    fixture.reopen_host().await?;
    let receiver_connections = second.enrolled_machine()?.session_connections();
    let target = target(second)?;
    run_scenario(
        start_peer(second, &target).await?,
        async |peer: &mut PeerFixture| {
            // Welcome's initial catch-up must finish before the only send.
            wire.wait_for(|events| events.iter().any(|event| matches!(event, MailWireEvent::StatusAcknowledged(_)))).await?;
            let connections = central_pair::central(fixture)?.session_connections();
            let baseline = wire.events().len();
            let sent = fixture.host()?.mcp("message_send", json!({
                "itemId":item,"target":target,"message":{"kind":"humanUser","text":"connected timer repair"},"delivery":"auto"
            })).await?;
            assert_eq!(sent.pointer("/queued/item"), Some(&json!(item)));
            peer.expect_marker("connected timer repair").await?;
            wire.wait_for(|events| events.iter().any(|event| matches!(event,
                MailWireEvent::Publication { publication, dropped: true } if publication.item == item && matches!(publication.status, DeliveryStatus::Delivered { .. })
            ))).await?;
            // No new send, explicit catch-up, Welcome or other mutation is used for repair.
            let repaired = tokio::time::timeout(Duration::from_secs(8), wire.wait_for(|events| {
                let Some(events) = events.get(baseline..) else { return false; };
                events.iter().any(|event| matches!(event, MailWireEvent::StatusFetched(batch)
                    if batch.events.iter().any(|event| event.item == item && matches!(event.status, DeliveryStatus::Delivered { .. }))
                    && events.iter().any(|event| matches!(event, MailWireEvent::StatusAcknowledged(through) if *through >= batch.through))))
            })).await;
            assert!(repaired.is_ok(), "connected timer never fetched and acknowledged the lost terminal status");
            repaired??;
            assert_eq!(show(fixture, item).await?.pointer("/mail/status/kind"), Some(&json!("delivered")));
            assert_eq!(central_pair::central(fixture)?.session_connections(), connections, "sender reconnected during repair");
            assert_eq!(second.enrolled_machine()?.session_connections(), receiver_connections, "recipient reconnected during repair");
            assert_eq!(peer.count_matching_after_barrier("connected timer repair").await?, 1);
            Ok(())
        },
        PeerFixture::shutdown,
        || Ok(()),
    ).await
}

pub(super) async fn start_peer(
    second: &SecondHost,
    target: &SessionRef,
) -> FixtureResult<PeerFixture> {
    use std::os::unix::fs::DirBuilderExt as _;
    let registry = second.directory.join("absent-peer-registry");
    match std::fs::DirBuilder::new().mode(0o700).create(&registry) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    PeerFixture::start_in_registry_with_session(
        &registry,
        &target.endpoint,
        target.session_id.clone(),
    )
    .await
}

pub(super) fn target(second: &SecondHost) -> FixtureResult<SessionRef> {
    Ok(SessionRef {
        endpoint: EndpointRef {
            service_id: second.service_id()?.try_into()?,
            endpoint_id: "claude-local".to_owned().try_into()?,
        },
        session_id: identity().try_into()?,
    })
}

async fn show(fixture: &BoardFixture, item: ItemId) -> FixtureResult<Value> {
    fixture
        .host()?
        .mcp(
            "router_show",
            json!({"caller":{"kind":"humanUser"},"reference":item}),
        )
        .await
}
