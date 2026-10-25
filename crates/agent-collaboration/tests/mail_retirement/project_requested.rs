//! FR1: the requested project-control owner retires its own canonical cancel only.
use crate::{
    central_pair::{self, SecondHost},
    fixture::{self, BoardFixture},
    lost_answer, readback,
};
use board_central_test_support::{MailWireEvent, ObjectAddress, RuntimeControl};
use codex_router_test_support::{
    FixtureResult, owned_peer_fixture::PeerFixture, scenario_finalization::run_scenario,
};
use collaboration_client::protocol::EndpointRef;
use fleet_sync_protocol::{DeliveryStatus, ItemId, ReportResult};
use serde_json::{Value, json};
use std::{os::unix::fs::DirBuilderExt, time::Duration};

async fn start_recipient(recipient: &SecondHost) -> FixtureResult<PeerFixture> {
    let registry = recipient.directory.join("absent-peer-registry");
    std::fs::DirBuilder::new().mode(0o700).create(&registry)?;
    let endpoint: EndpointRef = serde_json::from_value(
        json!({"serviceId":recipient.service_id()?,"endpointId":"claude-local"}),
    )?;
    PeerFixture::start_in_registry(&registry, &endpoint).await
}

async fn held_notification(sender: &BoardFixture, recipient: &SecondHost) -> FixtureResult<ItemId> {
    let mut wire = recipient.enrolled_machine()?.observe_mail_wire(None)?;
    let target = json!({"endpoint":{"serviceId":recipient.service_id()?,"endpointId":"claude-local"},"sessionId":"fixture-session"});
    sender.create().await?;
    let thread = sender.thread(&sender.project, &sender.owner).await?;
    let mut request = sender.reply_request(
        &sender.project,
        &thread,
        "requested project retirement marker",
    )?;
    request
        .as_object_mut()
        .ok_or("notification request absent")?
        .insert(
            "addressees".into(),
            json!([{"kind":"actor","recipient":{"kind":"session","session":target}}]),
        );
    sender.accept("thread_post", request).await?;
    wire.wait_for(|events| {
        events.iter().any(|event| {
            matches!(
                event,
                MailWireEvent::ReportAnswered {
                    answer: ReportResult::Applied {
                        status: DeliveryStatus::Held,
                        ..
                    },
                    ..
                }
            )
        })
    })
    .await?;
    let item = wire
        .events()
        .into_iter()
        .find_map(|event| match event {
            MailWireEvent::ReportAnswered {
                answer:
                    ReportResult::Applied {
                        item,
                        status: DeliveryStatus::Held,
                        ..
                    },
                ..
            } => Some(item),
            _ => None,
        })
        .ok_or("held project notification missing")?;
    central_pair::settled_view(sender.host()?, &sender.project).await?;
    Ok(item)
}

pub async fn settled_cancel(sender: &BoardFixture, request: &Value) -> FixtureResult<Value> {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let result = sender
                .host()?
                .mcp("project_notification_cancel", request.clone())
                .await?;
            if result.pointer("/outcome/kind") != Some(&json!("pending")) {
                return Ok(result);
            }
            tokio::task::yield_now().await;
        }
    })
    .await?
}

async fn project_counts(sender: &mut BoardFixture) -> FixtureResult<Value> {
    sender.stop_host().await?;
    let counts = readback::project_control_counts(sender).await;
    sender.reopen_host().await?;
    counts
}

async fn replay_after_restart(
    sender: &mut BoardFixture,
    request: Value,
    answer: &Value,
    before: &Value,
) -> FixtureResult {
    assert_eq!(
        &project_counts(sender).await?,
        before,
        "requested cancellation appended project custody or a request decision"
    );
    assert_eq!(
        settled_cancel(sender, &request).await?,
        *answer,
        "terminal requested-control replay changed after owning restart"
    );
    Ok(())
}

pub async fn retire_and_replay(
    sender: &mut BoardFixture,
    recipient: &mut SecondHost,
) -> FixtureResult {
    let item = held_notification(sender, recipient).await?;
    let before = project_counts(sender).await?;
    let cancel = fixture::identity();
    let request = sender.request(
        &sender.owner,
        json!({"kind":"notificationCancel","cancelId":cancel,"target":item}),
    );
    let answer = settled_cancel(sender, &request).await?;
    assert_eq!(answer.get("cancel"), Some(&json!(cancel)));
    assert_eq!(answer.pointer("/outcome/kind"), Some(&json!("prevented")));
    readback::await_compacted(sender, &recipient.service_id()?, &cancel).await?;
    replay_after_restart(sender, request.clone(), &answer, &before).await?;
    assert_prevented_peer(sender, recipient, item, request, answer).await
}

pub async fn lost_answer(sender: &mut BoardFixture, recipient: &mut SecondHost) -> FixtureResult {
    let item = held_notification(sender, recipient).await?;
    let before = project_counts(sender).await?;
    let cancel = fixture::identity();
    let request = sender.request(
        &sender.owner,
        json!({"kind":"notificationCancel","cancelId":cancel,"target":item}),
    );
    run_scenario(
        sender,
        async |sender: &mut &mut BoardFixture| {
            let destination = ObjectAddress::Mailbox(recipient.service_id()?.try_into()?);
            let central = central_pair::central(sender)?;
            central
                .runtime_control(
                    ObjectAddress::Registry,
                    RuntimeControl::HoldNextDispatch {
                        destination: destination.clone(),
                    },
                )
                .await?;
            let pending = sender
                .host()?
                .mcp("project_notification_cancel", request.clone())
                .await?;
            assert_eq!(pending.get("cancel"), Some(&json!(cancel)));
            lost_answer::hold_retirement_after_submit(central, destination.clone()).await?;
            let retirement =
                held_requested_retirement(central, destination.clone(), &cancel).await?;
            lost_answer::lose_reply_and_hold_retry(central, destination.clone()).await?;
            assert_eq!(
                held_requested_retirement(central, destination.clone(), &cancel).await?,
                retirement
            );
            readback::require_compacted(sender, &recipient.service_id()?, &cancel).await?;
            sender.stop_host().await?;
            central_pair::central(sender)?
                .runtime_control(
                    ObjectAddress::Registry,
                    RuntimeControl::ResumeDispatchAndHoldNext {
                        destination: destination.clone(),
                    },
                )
                .await?;
            sender.reopen_host().await?;
            assert_eq!(
                held_requested_retirement(central_pair::central(sender)?, destination, &cancel)
                    .await?,
                retirement
            );
            central_pair::central(sender)?
                .runtime_control(ObjectAddress::Registry, RuntimeControl::ResumeDispatch)
                .await?;
            let answer = settled_cancel(sender, &request).await?;
            assert_eq!(answer.pointer("/outcome/kind"), Some(&json!("prevented")));
            replay_after_restart(sender, request.clone(), &answer, &before).await?;
            assert_prevented_peer(sender, recipient, item, request.clone(), answer).await
        },
        |sender| lost_answer::release_if_held(central_pair::central(sender)),
        || Ok(()),
    )
    .await
}

async fn held_requested_retirement(
    central: &board_central_test_support::TestCentral,
    destination: ObjectAddress,
    cancel: &str,
) -> FixtureResult<Vec<u8>> {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let held = central
                .runtime_control(ObjectAddress::Registry, RuntimeControl::AwaitHeldDispatch)
                .await
                .map_err(|error| {
                    format!("requested project cancel never dispatched retirement: {error}")
                })?;
            let board_central_test_support::DispatchFaultObservation::Held { request, .. } =
                held.dispatch
            else {
                return Err("expected held project mail call".into());
            };
            let envelope: Value = serde_json::from_slice(&request)?;
            if envelope.pointer("/request/request/kind") == Some(&json!("retire"))
                && envelope.pointer("/request/request/value/items") == Some(&json!([cancel]))
            {
                return Ok(request);
            }
            // The original's separate project-frontier Query/Retire can run concurrently.
            central
                .runtime_control(
                    ObjectAddress::Registry,
                    RuntimeControl::ResumeDispatchAndHoldNext {
                        destination: destination.clone(),
                    },
                )
                .await?;
        }
    })
    .await?
}

async fn assert_prevented_peer(
    sender: &BoardFixture,
    recipient: &SecondHost,
    item: ItemId,
    request: Value,
    answer: Value,
) -> FixtureResult {
    run_scenario(
        start_recipient(recipient).await?,
        async |peer: &mut PeerFixture| {
            recipient
                .host()?
                .mcp(
                    "thread_subscriptions",
                    json!({"actor":{"kind":"session","session":peer.target}}),
                )
                .await?;
            assert_eq!(
                peer.count_matching_after_barrier(&item.as_uuid().to_string())
                    .await?,
                0
            );
            assert_eq!(settled_cancel(sender, &request).await?, answer);
            Ok(())
        },
        PeerFixture::shutdown,
        || Ok(()),
    )
    .await
}
