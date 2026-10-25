//! Forwarded admission must wake the holder's existing native publication owners.
use super::*;
use board_central_test_support::{DispatchFaultObservation, ObjectAddress, RuntimeControl};
use collaboration_protocol::{PushDeliveryState, PushHeaderFacts};
use fleet_sync_protocol::{DatabaseId, ItemId};
use message_board_storage::MailHandoff;

async fn cause_intents(
    holder: &claim_fixture::ClaimFixture,
    project: &ProjectId,
    post: &PostId,
) -> Result<Vec<NotifyIntent>, Box<dyn std::error::Error + Send + Sync>> {
    let store = holder
        .registry
        .existing_project(project)
        .await?
        .ok_or("holder file")?;
    let mut read = store.snapshot().await?;
    let intents = read
        .records(RecordKind::NotifyIntent)
        .await?
        .into_iter()
        .filter_map(|record| match record {
            Record::NotifyIntent(intent) if intent.cause == post.clone().into() => Some(intent),
            _ => None,
        })
        .collect();
    read.finish().await?;
    Ok(intents)
}

fn actor_service(intent: &NotifyIntent) -> Option<ServiceId> {
    match intent.recipient() {
        IntentRecipient::Resolved(recipient) => match recipient.actor.declared_identity() {
            Some(Identity::Session { session }) => Some(session.endpoint.service_id),
            _ => None,
        },
        _ => None,
    }
}

async fn frozen_handoff(
    holder: &claim_fixture::ClaimFixture,
    project: &ProjectId,
    intent: &NotifyIntent,
) -> Result<MailHandoff, Box<dyn std::error::Error + Send + Sync>> {
    let IntentState::HandedOff { item } = intent.state() else {
        return Err("intent not frozen".into());
    };
    let store = holder
        .registry
        .existing_project(project)
        .await?
        .ok_or("holder file")?;
    let mut read = store.snapshot().await?;
    let handoff = read
        .mail_handoff(item)
        .await?
        .ok_or("native frozen handoff")?;
    read.finish().await?;
    Ok(handoff)
}

fn item_id(intent: &NotifyIntent) -> Result<ItemId, Box<dyn std::error::Error + Send + Sync>> {
    let IntentState::HandedOff { item } = intent.state() else {
        return Err("intent not frozen".into());
    };
    Ok(ItemId::try_from(item.as_str().to_owned())?)
}

#[tokio::test]
async fn non_holder_sender_publishes_local_notifications_before_copy_ack_and_deduplicates_retry()
-> TestResult {
    let holder = claim_fixture::ClaimFixture::start_with_notifications().await?;
    let project = claimed_project(&holder).await?;
    let sender = SenderFixture::start(&holder).await?;
    let session = ObjectAddress::Connection(origin()?.installation);
    let copy = ObjectAddress::Copy(DatabaseId::try_from(project.as_str().to_owned())?);
    let proof = async {
        let notifications = holder.notifications.as_ref().ok_or("native holder notifications")?;
        let request = post_request(&project)?;
        grant_posting_role(&holder, &request).await?;
        let store = holder.registry.existing_project(&project).await?.ok_or("holder file")?;
        timeout(BOUND, async {
            loop {
                notifications.publisher.queue_project(project.clone()).await?;
                notifications.publisher.wait_project_idle_for_test(&project).await;
                let read = store.snapshot().await?;
                let head = read.high_water();
                read.finish().await?;
                if notifications.owner.intents_scanned_through(&project).await? == Some(head)
                    && holder.projects.replicated_through(&project, head.epoch).await?.position >= head.position {
                    return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
                }
                tokio::task::yield_now().await;
            }
        }).await??;
        assert!(notifications.rows().await?.iter().all(|row| row.category != "inbox" && row.category != "outbox"));

        let BoardOperation::ThreadCreate { post_id, about, text, participation, watch, .. } = request.operation else {
            return Err("post fixture".into());
        };
        let local_service = origin()?.installation;
        let recipient = |service: ServiceId| -> Result<NotifyTarget, Box<dyn std::error::Error + Send + Sync>> {
            Ok(NotifyTarget::Actor(Identity::Session { session: SessionRef {
                endpoint: SessionEndpointRef { service_id: service, endpoint_id: "codex-local".to_owned().try_into()? },
                session_id: "forwarded-notification-recipient".to_owned().try_into()?,
            } }))
        };
        let create = ThreadCreateRequest {
            request_id: request.request_id, protocol_version: request.protocol_version,
            project_id: project.clone(), post_id: post_id.clone(), about,
            actor: request.actor, acting_for: request.acting_for, text,
            references: serde_json::from_value(serde_json::json!([]))?, role: participation, watch,
            addressees: vec![recipient(local_service.clone())?, recipient(sender.origin.installation.clone())?],
        };
        holder.central.runtime_control(session.clone(), RuntimeControl::HoldNextDispatch { destination: copy.clone() }).await?;
        // Hold only progress storage until the cause piece is captured. This
        // forces a distinct later handoff ACK without replacing native Copy.
        let (release_progress, progress_task) = notifications.hold_progress_writer().await?;
        let posting = {
            let application = sender.application.clone();
            let create = create.clone();
            tokio::spawn(async move { application.board().thread_create(create).await })
        };
        let entered = holder.central.runtime_control(session.clone(), RuntimeControl::AwaitHeldDispatch).await;
        let _ = release_progress.send(());
        timeout(BOUND, progress_task).await???;
        assert!(matches!(entered?.dispatch, DispatchFaultObservation::Held { destination, .. } if destination == copy));
        let accepted = timeout(BOUND, posting).await???;
        assert!(matches!(accepted.result, message_board::public_replies::RequestResult::Decided(
            message_board::public_replies::RequestDecision::Accepted { .. }
        )), "forwarded notification post must be admitted: {accepted:?}");

        let (local, remote, early_push) = timeout(std::time::Duration::from_secs(5), async {
            loop {
                let intents = cause_intents(&holder, &project, &post_id).await?;
                let local = intents.iter().find(|intent| actor_service(intent).as_ref() == Some(&local_service));
                let remote = intents.iter().find(|intent| actor_service(intent).as_ref() == Some(&sender.origin.installation));
                if let (Some(local), Some(remote)) = (local, remote)
                    && matches!(local.state(), IntentState::HandedOff { .. })
                    && matches!(remote.state(), IntentState::HandedOff { .. })
                    && let Some(push) = notifications.local_push(item_id(local)?).await? {
                    return Ok::<_, Box<dyn std::error::Error + Send + Sync>>((local.clone(), remote.clone(), push));
                }
                tokio::task::yield_now().await;
            }
        }).await.map_err(|_| io::Error::other("forwarded holder-local custody waited for Copy ACK"))??;
        let local_item = item_id(&local)?;
        let remote_item = item_id(&remote)?;
        let local_handoff = frozen_handoff(&holder, &project, &local).await?;
        let remote_handoff = frozen_handoff(&holder, &project, &remote).await?;
        assert_eq!(early_push.record.push_id.as_str(), local_item.as_uuid().to_string());
        assert_eq!(early_push.record.delivery_state, PushDeliveryState::Held);
        assert!(early_push.record.body.is_none());
        assert_eq!(early_push.record.header_facts, PushHeaderFacts::BoardNotification {
            project: project.clone(), intent: local.intent_id.clone(), cause: post_id.clone().into(),
            summary: "New thread post".to_owned().try_into()?,
        });
        assert_eq!(early_push.link, format!("router://{}/push/{}", local_service.as_str(), local_item.as_uuid()));
        let before = notifications.rows().await?;
        assert_eq!(before.iter().filter(|row| row.category == "inbox" && row.identity == local_item.as_uuid().to_string()).count(), 1);
        assert!(!before.iter().any(|row| row.category == "outbox" && row.identity == remote_item.as_uuid().to_string()), "remote custody bypassed the cause ACK");
        let cause_marks = holder.projects.acknowledged_mail_changes(&project).await?;
        assert!(cause_marks.max_handoff_seq.is_none());
        let mut read = store.snapshot().await?;
        let cause = read.record(&post_id.clone().into()).await?.ok_or("admitted post")?;
        let cause_position = cause.envelope().position.project_cursor().ok_or("cause position")?;
        read.finish().await?;
        assert!(holder.projects.replicated_through(&project, cause_position.epoch).await?.position < cause_position.position);
        let replay = sender.application.board().thread_create(create).await?;
        assert_eq!(replay.result, accepted.result);
        timeout(BOUND, notifications.publisher.wait_project_idle_for_test(&project)).await?;
        let retried = cause_intents(&holder, &project, &post_id).await?;
        assert_eq!(retried.len(), 2, "keyed retry allocated another intent");
        for intent in &retried {
            let expected = if actor_service(intent).as_ref() == Some(&local_service) { local_item } else { remote_item };
            assert_eq!(item_id(intent)?, expected, "keyed retry changed frozen mail identity");
        }
        holder.central.runtime_control(session.clone(), RuntimeControl::ResumeDispatchAndHoldNext { destination: copy.clone() }).await?;
        assert!(matches!(holder.central.runtime_control(session.clone(), RuntimeControl::AwaitHeldDispatch).await?.dispatch,
            DispatchFaultObservation::Held { destination, .. } if destination == copy));
        assert!(holder.projects.replicated_through(&project, cause_position.epoch).await?.position >= cause_position.position);
        assert!(holder.projects.acknowledged_mail_changes(&project).await?.max_handoff_seq.is_none());
        assert!(!notifications.rows().await?.iter().any(|row| row.category == "outbox" && row.identity == remote_item.as_uuid().to_string()), "remote custody bypassed the frozen handoff ACK");
        holder.central.runtime_control(session.clone(), RuntimeControl::ResumeDispatch).await?;
        timeout(BOUND, async {
            loop {
                let marks = holder.projects.acknowledged_mail_changes(&project).await?;
                let rows = notifications.rows().await?;
                if marks.max_handoff_seq.is_some_and(|sequence| sequence >= remote_handoff.sequence.get())
                    && rows.iter().any(|row| row.category == "outbox" && row.identity == remote_item.as_uuid().to_string()) {
                    return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(rows);
                }
                tokio::task::yield_now().await;
            }
        }).await??;
        timeout(BOUND, notifications.publisher.wait_project_idle_for_test(&project)).await?;
        let final_rows = notifications.rows().await?;
        assert_eq!(final_rows.iter().filter(|row| row.category == "inbox" && row.identity == local_item.as_uuid().to_string()).count(), 1);
        assert_eq!(final_rows.iter().filter(|row| row.category == "outbox" && row.identity == remote_item.as_uuid().to_string()).count(), 1);
        let final_local = frozen_handoff(&holder, &project, &local).await?;
        assert_eq!(final_local.sequence, local_handoff.sequence);
        assert_eq!(serde_json::to_value(final_local.item)?, serde_json::to_value(local_handoff.item)?);
        let final_remote = frozen_handoff(&holder, &project, &remote).await?;
        assert_eq!(final_remote.sequence, remote_handoff.sequence);
        assert_eq!(serde_json::to_value(final_remote.item)?, serde_json::to_value(remote_handoff.item)?);
        let final_push = notifications.local_push(local_item).await?.ok_or("local custody disappeared")?;
        assert_eq!(final_push.link, early_push.link);
        assert_eq!(final_push.record.push_id, early_push.record.push_id);
        assert_eq!(final_push.record.header_facts, early_push.record.header_facts);
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    // Release the real dispatcher even when the missing-prompt baseline fails.
    let _ = holder
        .central
        .runtime_control(session, RuntimeControl::ResumeDispatch)
        .await;
    let sender_closed = sender.shutdown().await;
    let holder_closed = holder.shutdown().await;
    proof?;
    sender_closed?;
    holder_closed?;
    Ok(())
}
