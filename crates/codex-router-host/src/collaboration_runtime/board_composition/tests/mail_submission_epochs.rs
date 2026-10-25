//! Native epoch custody crosses the real SessionPump after every owning reopen.
use super::*;
use transport::WireEvent;

#[derive(Debug, Eq, PartialEq)]
enum SubmissionAttempt {
    Sent { epoch: Option<u64> },
    RetriedWithoutSubmission,
}

async fn first_submit_attempt(
    harness: &MailHarness,
    item: fleet_sync_protocol::ItemId,
    operation: runtime_primitives::OperationId,
) -> Result<SubmissionAttempt, TestError> {
    tokio::time::timeout(BOUND, async {
        loop {
            if let Some(epoch) = harness.transport.observer.events().iter().find_map(|event| {
                match event {
                    WireEvent::SubmissionEpoch { item: sent, epoch } if *sent == item => Some(*epoch),
                    _ => None,
                }
            }) { return Ok(SubmissionAttempt::Sent { epoch }); }
            let (record, _) = harness.mail.operation(operation).await?;
            if record.is_some_and(|record| matches!(record.execution,
                durable_steps::storage::OperationExecution::Running { attempt, drive: None, .. } if attempt >= 1)) {
                return Ok(SubmissionAttempt::RetriedWithoutSubmission);
            }
            tokio::task::yield_now().await;
        }
    }).await.map_err(|_| io::Error::other("project submission attempt did not reach a wire or retry outcome"))?
}

#[tokio::test]
async fn pending_project_submission_uses_its_recorded_epoch_after_owner_restart()
-> Result<(), TestError> {
    let directory = tempfile::tempdir()?;
    let mut first = MailHarness::open_manual(directory.path(), None).await?;
    let peer = first.peer();
    let project = message_board::ProjectId::generate();
    let transfer = first.establish_epoch_two(&project).await?;
    let item = relay_items::ordinary_project_notification(peer, project.clone());
    let (item, operation) = first.mail.enqueue_with_epoch(item, 2.try_into()?).await?;
    first.move_to_epoch_three(&project, &transfer).await?;
    transfer.close().await?;
    first.release_welcome()?;
    assert!(matches!(
        first.next_manual_event().await?,
        ClientEvent::Welcome { .. }
    ));
    first.close().await?;

    let mut resumed = MailHarness::open_manual(directory.path(), Some(peer)).await?;
    // The Host has no current project writer here. Only the native recorded
    // authority can recover this pending item; central policy remains G3's.
    resumed.start_mail();
    let sent = first_submit_attempt(&resumed, item, operation).await?;
    resumed.release_welcome()?;
    resumed.close().await?;
    assert_eq!(
        sent,
        SubmissionAttempt::Sent { epoch: Some(2) },
        "reopened Project mail lost its frozen submission epoch"
    );
    Ok(())
}

#[tokio::test]
async fn a_new_project_admission_uses_its_own_epoch_and_missing_authority_never_sends()
-> Result<(), TestError> {
    let directory = tempfile::tempdir()?;
    let mut harness = MailHarness::open_manual(directory.path(), None).await?;
    let valid = relay_items::ordinary_notification(harness.peer());
    let (valid, valid_operation) = harness
        .mail
        .enqueue_with_epoch(valid, 4.try_into()?)
        .await?;
    // Deliberately malformed local state: real SDK custody exists, but the
    // board's required companion was not admitted with it.
    let missing = relay_items::ordinary_notification(harness.peer());
    let (missing, missing_operation) = harness.mail.enqueue(missing, harness.relay).await?;
    harness.start_mail();
    let sent = first_submit_attempt(&harness, valid, valid_operation).await?;
    let retried = first_submit_attempt(&harness, missing, missing_operation).await?;
    let events = harness.transport.observer.events();
    harness.release_welcome()?;
    harness.close().await?;
    assert_eq!(sent, SubmissionAttempt::Sent { epoch: Some(4) });
    assert_eq!(retried, SubmissionAttempt::RetriedWithoutSubmission);
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, WireEvent::MailSubmit(item) if *item == missing))
    );
    Ok(())
}

#[tokio::test]
async fn session_and_human_submissions_carry_no_project_epoch() -> Result<(), TestError> {
    let directory = tempfile::tempdir()?;
    let mut harness = MailHarness::open_manual(directory.path(), None).await?;
    let human = relay_items::ordinary_message(harness.peer());
    let session = fleet_sync_protocol::DeliveryItem::new(
        fleet_sync_protocol::ItemId::new(uuid::Uuid::now_v7())?,
        collaboration_mail_protocol::MailSender::Session(human.recipient().address.clone()),
        human.recipient().clone(),
        None,
        human.body().clone(),
    )?;
    let (human, human_operation) = harness.mail.enqueue(human, harness.relay).await?;
    let (session, session_operation) = harness.mail.enqueue(session, harness.relay).await?;
    harness.start_mail();
    let human = first_submit_attempt(&harness, human, human_operation).await?;
    let session = first_submit_attempt(&harness, session, session_operation).await?;
    harness.release_welcome()?;
    harness.close().await?;
    assert_eq!(human, SubmissionAttempt::Sent { epoch: None });
    assert_eq!(session, SubmissionAttempt::Sent { epoch: None });
    Ok(())
}
