use super::{
    central_pair::{self, SecondHost},
    connected_status,
    fixture::{BoardFixture, identity},
    owned_host::BoardHost,
};
use board_central_test_support::{MailWireEvent, MailWireObservation};
use codex_router_test_support::{
    FixtureResult, owned_peer_fixture::PeerFixture, scenario_finalization::run_scenario,
};
use fleet_sync_protocol::{DeliveryStatus, ItemId, RecipientOutcome, ReportResult, TransferId};
use serde_json::{Value, json};

pub async fn remote_original(fixture: &mut BoardFixture, second: &mut SecondHost) -> FixtureResult {
    fixture.create().await?;
    let target = connected_status::target(second)?;
    let mut recipient_wire = second.enrolled_machine()?.observe_mail_wire(None)?;
    let thread = fixture.thread(&fixture.project, &fixture.owner).await?;
    let mut post =
        fixture.reply_request(&fixture.project, &thread, "held remote original to cancel")?;
    post.as_object_mut().ok_or("post is not an object")?.insert(
        "addressees".into(),
        json!([{"kind":"actor","recipient":{"kind":"session","session":target}}]),
    );
    fixture.accept("thread_post", post).await?;
    let item = held_original(&mut recipient_wire).await?;
    central_pair::settled_view(fixture.host()?, &fixture.project).await?;
    let successor = second.service_id()?.try_into()?;
    let transfer = central_pair::central(fixture)?
        .move_project(
            fixture.project.clone().try_into()?,
            &successor,
            TransferId::new(uuid::Uuid::now_v7())?,
        )
        .await?;
    assert_eq!(transfer.get("kind"), Some(&json!("moved")));
    central_pair::await_writer(second.host()?, &fixture.project, 2).await?;
    let request = fixture.request(
        &fixture.owner,
        json!({"kind":"notificationCancel","cancelId":identity(),"target":item}),
    );
    let cancelled = settled_cancel(second.host()?, &request).await?;
    assert_eq!(
        cancelled.pointer("/outcome/kind"),
        Some(&json!("prevented"))
    );
    let reported = recipient_wire
        .wait_for(|events| prevented_report(events, item))
        .await;
    let report_sequences = report_sequences(&recipient_wire.events(), item);
    assert!(
        reported.is_ok(),
        "local prevention never reached central as report sequence 2: {reported:?}; sent sequences (prevented)={report_sequences:?}"
    );
    reported?;
    let control: ItemId = serde_json::from_value(
        cancelled
            .get("cancel")
            .cloned()
            .ok_or("cancel identity absent")?,
    )?;
    recipient_wire.wait_for(|events| events.iter().any(|event| matches!(event,
        MailWireEvent::ReportAnswered { answer: ReportResult::Applied { item, acknowledged, status: DeliveryStatus::Cancelled, .. }, .. }
        if *item == control && acknowledged.value() == 1
    ))).await?;
    second.stop_host().await?;
    second.reopen_host().await?;
    let replay = second
        .host()?
        .mcp("project_notification_cancel", request)
        .await?;
    assert_eq!(
        replay, cancelled,
        "same-ID local cancellation changed after restart"
    );
    run_scenario(
        connected_status::start_peer(second, &target).await?,
        async |peer: &mut PeerFixture| {
            second
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
            // Re-read after the native peer barrier; replay cannot change the canonical outcome.
            assert!(prevented_report(&recipient_wire.events(), item));
            Ok(())
        },
        PeerFixture::shutdown,
        || Ok(()),
    )
    .await
}

async fn settled_cancel(host: &BoardHost, request: &Value) -> FixtureResult<Value> {
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        loop {
            let result = host
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

fn report_sequences(events: &[MailWireEvent], expected: ItemId) -> Vec<(u64, bool)> {
    events
        .iter()
        .filter_map(|event| match event {
            MailWireEvent::ReportSent {
                item,
                sequence,
                outcome,
                ..
            } if *item == expected => Some((
                sequence.value(),
                matches!(outcome, RecipientOutcome::Prevented { .. }),
            )),
            _ => None,
        })
        .collect()
}

async fn held_original(wire: &mut MailWireObservation) -> FixtureResult<ItemId> {
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
    wire.events()
        .iter()
        .find_map(|event| match event {
            MailWireEvent::ReportAnswered {
                answer:
                    ReportResult::Applied {
                        item,
                        status: DeliveryStatus::Held,
                        ..
                    },
                ..
            } => Some(*item),
            _ => None,
        })
        .ok_or_else(|| "no real remote-origin held item".into())
}

fn prevented_report(events: &[MailWireEvent], expected: ItemId) -> bool {
    events.iter().any(|event| match event {
        MailWireEvent::ReportSent { frame, item, sequence, outcome: RecipientOutcome::Prevented { .. } }
            if *item == expected && sequence.value() == 2 => events.iter().any(|event| matches!(event,
                MailWireEvent::ReportAnswered { frame: answer_frame, answer: ReportResult::Applied { item, acknowledged, status: DeliveryStatus::Cancelled, .. } }
                if answer_frame == frame && *item == expected && acknowledged.value() == 2)),
        _ => false,
    })
}
