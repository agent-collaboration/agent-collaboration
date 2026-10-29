//! A successor admits the inherited original and its cancel before its SDK worker starts.
use super::*;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions follow fallible real-fixture setup."
)]
async fn inherited_handoff_cancel_reaches_the_remote_hold_before_any_local_submit_task()
-> TestResult {
    let mut fixture = Box::pin(RuleFixture::open_with_deferred_successor()).await?;
    let proof = Box::pin(async {
        let seed = Box::pin(arrange_sent_follow(&mut fixture)).await?;
        let cancel = hold_old_cancel_away_from_mailbox(&fixture, &seed).await?;
        publish_inherited_prefix_with_worker_deferred(&mut fixture).await?;

        // No successor mail worker has run. Cancellation admission must not
        // locally settle an original that the old holder already left Held.
        let rows = fixture.successor.notifications.rows().await?;
        let original = rows
            .iter()
            .find(|row| {
                row.category == "outbox" && row.identity == seed.item.id().as_uuid().to_string()
            })
            .ok_or("successor original custody")?;
        let original: serde_json::Value =
            serde_json::from_str(original.content.as_deref().ok_or("original header")?)?;
        assert_eq!(
            original.pointer("/value/status"),
            Some(&json!({"kind":"queued"})),
            "inherited original was withdrawn before the successor could send its cancel"
        );
        let control = rows
            .iter()
            .find(|row| {
                row.category == "outbox" && row.identity == cancel.id().as_uuid().to_string()
            })
            .ok_or("successor cancel custody")?;
        let control: serde_json::Value =
            serde_json::from_str(control.content.as_deref().ok_or("cancel header")?)?;
        assert_eq!(
            control.pointer("/value/cancel"),
            Some(&json!({"kind":"pending"}))
        );

        fixture.successor.notifications.start_mail_worker()?;
        let controls = wait_fetch(&mut fixture, "controls").await?;
        assert_eq!(
            controls.pointer("/value/value/entries"),
            Some(&json!([{"kind":"item","value":{"seq":1,"item":cancel}}]))
        );
        decide_and_replay(&mut fixture, &seed, &cancel).await?;
        let fetched = operation(
            &mut fixture.recipient.socket,
            relay_request(
                "fetch",
                json!({"stream":"items","after":1,"maxItems":10,"maxBytes":1_000_000}),
            ),
        )
        .await?;
        assert_eq!(fetched.pointer("/value/value/entries"), Some(&json!([])));
        assert_eq!(fetched.pointer("/value/value/through"), Some(&json!(1)));
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    })
    .await;
    let _ = fixture
        .holder
        .central
        .runtime_control(ObjectAddress::Registry, RuntimeControl::ResumeDispatch)
        .await;
    let closed = fixture.close().await;
    closed?;
    proof
}

async fn hold_old_cancel_away_from_mailbox(
    fixture: &RuleFixture,
    seed: &FollowSeed,
) -> Result<DeliveryEnvelope<BoardDeliveryTypes>, Box<dyn std::error::Error + Send + Sync>> {
    // Only the successor may finish the remote prevention asserted by this test.
    fixture
        .holder
        .central
        .runtime_control(
            ObjectAddress::Registry,
            RuntimeControl::HoldNextSelectedDispatch {
                destination: ObjectAddress::Mailbox(rule_fixture::RECIPIENT.to_owned().try_into()?),
                call: DispatchCallSelector::MailboxSubmit,
            },
        )
        .await?;
    let (_group, cancel) = arrange_remote_end(fixture, seed, CancelPrefix::Retained).await?;
    let held = fixture
        .holder
        .central
        .runtime_control(ObjectAddress::Registry, RuntimeControl::AwaitHeldDispatch)
        .await?;
    let board_central_test_support::DispatchFaultObservation::Held { request, .. } = held.dispatch
    else {
        return Err("old holder cancel was not held".into());
    };
    let request: serde_json::Value = serde_json::from_slice(&request)?;
    assert_eq!(
        request.pointer("/request/item/kind"),
        Some(&json!("cancel"))
    );
    assert_eq!(
        request.pointer("/request/item/value/id"),
        Some(&json!(cancel.id()))
    );
    Ok(cancel)
}

async fn publish_inherited_prefix_with_worker_deferred(fixture: &mut RuleFixture) -> TestResult {
    fixture.holder.disconnect_holder().await?;
    let moved = fixture
        .holder
        .central
        .take_over_project(
            DatabaseId::try_from(fixture.project.as_str().to_owned())?,
            &fixture.successor_origin.installation,
            TransferId::new(durable_steps::Uuid::now_v7())?,
        )
        .await?;
    assert_eq!(moved.pointer("/value/newEpoch"), Some(&json!(3)));
    received_grants::wait_published_project(&fixture.successor.registry, &fixture.project, 3)
        .await?;
    timeout(BOUND, async {
        loop {
            fixture
                .successor
                .projects
                .replicate_project_for_test(&fixture.project)
                .await?;
            let marks = fixture
                .successor
                .projects
                .acknowledged_mail_changes(&fixture.project)
                .await?;
            if marks.max_handoff_seq == Some(2) && marks.max_expansion_seq == Some(1) {
                break;
            }
            tokio::task::yield_now().await;
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    })
    .await??;
    fixture
        .successor
        .notifications
        .publisher
        .queue_project(fixture.project.clone())
        .await?;
    timeout(
        BOUND,
        fixture
            .successor
            .notifications
            .publisher
            .wait_project_idle_for_test(&fixture.project),
    )
    .await?;
    Ok(())
}
