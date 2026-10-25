//! Real remote operations must leave capacity available for the next operation.
use crate::{
    central_pair::SecondHost,
    fixture::{self, BoardFixture},
    readback,
};
use codex_router_test_support::{
    FixtureResult, owned_peer_fixture::PeerFixture, scenario_finalization::run_scenario,
};
use collaboration_client::protocol::EndpointRef;
use serde_json::{Value, json};
use std::os::unix::fs::DirBuilderExt;

pub async fn start_peer(
    fixture: &BoardFixture,
    endpoint: &EndpointRef,
) -> FixtureResult<PeerFixture> {
    let registry = fixture.directory.join("absent-peer-registry");
    std::fs::DirBuilder::new().mode(0o700).create(&registry)?;
    PeerFixture::start_in_registry(&registry, endpoint).await
}

pub fn sender(host: &SecondHost) -> FixtureResult<Value> {
    Ok(
        json!({"endpoint":{"serviceId":host.service_id()?,"endpointId":"codex-local"},"sessionId":"retirement-sender"}),
    )
}

/// A connected Access socket can precede Welcome; replay only the same operation.
pub async fn call(
    host: &crate::owned_host::BoardHost,
    tool: &str,
    request: Value,
) -> FixtureResult<Value> {
    tokio::time::timeout(std::time::Duration::from_secs(30), async {
        loop {
            let reply = host.mcp_reply(tool, request.clone()).await?;
            if !reply.is_error {
                return Ok(reply.value);
            }
            if !matches!(
                reply
                    .value
                    .pointer("/data/kind")
                    .or_else(|| reply.value.get("kind"))
                    .and_then(Value::as_str),
                Some("unavailable" | "remoteUnavailable")
            ) {
                return Err(format!("mail front door refused: {}", reply.value).into());
            }
            tokio::task::yield_now().await;
        }
    })
    .await?
}

pub async fn capacity(fixture: &mut BoardFixture, sender_host: &mut SecondHost) -> FixtureResult {
    let service: String = fixture
        .installation
        .as_ref()
        .ok_or("recipient absent")?
        .service_id
        .clone()
        .into();
    let endpoint: EndpointRef =
        serde_json::from_value(json!({"serviceId":service,"endpointId":"claude-local"}))?;
    run_scenario(start_peer(fixture, &endpoint).await?, async |peer: &mut PeerFixture| {
        let sender=sender(sender_host)?;
        let mut previous: Option<String>=None;
        for index in 0..3 {
            let compacted=match &previous {Some(item)=>readback::await_compacted(fixture,&service,item).await,None=>Ok(())};
            let item=fixture::identity();
            call(sender_host.host()?,"message_send",json!({"itemId":item,"target":peer.target,
                "message":{"kind":"agent","sender":sender,"text":format!("capacity marker {index}")},"delivery":"auto"})).await?;
            let status=readback::terminal(sender_host.host()?,&sender,&item).await?;
            assert_eq!(status.get("kind"),Some(&json!("delivered")),"terminal backlog consumed the only mailbox slot: {status}");
            compacted?;
            peer.expect_marker(&format!("capacity marker {index}")).await?;
            previous=Some(item);
        }
        assert_eq!(peer.count_matching_after_barrier("capacity marker").await?,3);
        readback::await_compacted(fixture,&service,&previous.ok_or("last item")?).await
    }, PeerFixture::shutdown, || Ok(())).await
}

pub async fn wakes(fixture: &mut BoardFixture, sender_host: &mut SecondHost) -> FixtureResult {
    let service: String = fixture
        .installation
        .as_ref()
        .ok_or("recipient absent")?
        .service_id
        .clone()
        .into();
    let target = json!({"endpoint":{"serviceId":service,"endpointId":"claude-local"},"sessionId":"retirement-wake-recipient"});
    let operation = fixture::identity();
    let created=call(sender_host.host()?,"wake_send",json!({"operationId":operation,"requestedBy":{"kind":"owner"},"replace":false,
        "message":{"target":target,"content":{"kind":"humanUser","text":"retirement wake marker"},"delivery":"auto"},
        "timing":{"kind":"after","seconds":3600},"expiry":{"kind":"none"}})).await?;
    let wake = created
        .pointer("/definition/wakeupId")
        .ok_or_else(|| format!("wake identity missing: {created}"))?
        .clone();
    readback::await_compacted(fixture, &service, &operation).await?;
    for (tool, state) in [
        ("wake_pause", "paused"),
        ("wake_resume", "active"),
        ("wake_cancel", "cancelled"),
    ] {
        let operation = fixture::identity();
        let result = sender_host
            .host()?
            .mcp(tool, json!({"operationId":operation,"wakeupId":wake}))
            .await?;
        assert_eq!(
            result.pointer("/wakeup/state"),
            Some(&json!(state)),
            "wake mutation did not apply: {result}"
        );
        readback::await_compacted(fixture, &service, &operation).await?;
    }
    Ok(())
}
