//! Existing sender custody runs after an owning reopen, before any new Welcome admission.
use super::*;
use fleet_sync_protocol::ItemStatusState;
use transport::WireEvent;

#[tokio::test]
async fn cold_mail_owner_recovers_the_original_submission_without_a_new_receive_admission()
-> Result<(), TestError> {
    let directory = tempfile::tempdir()?;
    let mut first = MailHarness::open_manual(directory.path(), None).await?;
    let peer = first.peer();
    let item = relay_items::ordinary_message(peer);
    let digest = item.digest().clone();
    let (item, operation) = first.mail.enqueue(item, first.relay).await?;
    let before = first
        .mail
        .operation(operation)
        .await?
        .0
        .ok_or("original submission missing")?;
    assert!(matches!(
        before.execution,
        durable_steps::storage::OperationExecution::Running {
            attempt: 0,
            drive: None,
            ..
        }
    ));
    assert!(first.mail.state().await?.is_none());
    assert!(
        !first
            .transport
            .observer
            .events()
            .iter()
            .any(|event| matches!(event, WireEvent::MailSubmit(_)))
    );
    first.release_welcome()?;
    assert!(matches!(
        first.next_manual_event().await?,
        ClientEvent::Welcome { .. }
    ));
    // No runner was activated in the first lifetime. Close every owner and
    // session before constructing the new runtime and engine over the same file.
    first.close().await?;

    let mut resumed = MailHarness::open_manual(directory.path(), Some(peer)).await?;
    assert!(resumed.mail.state().await?.is_none());
    assert!(
        !*resumed.welcomed.borrow(),
        "fresh Host Welcome was not held"
    );
    let before_drive = resumed
        .mail
        .operation(operation)
        .await?
        .0
        .ok_or("owning reopen lost the original SDK operation")?;
    assert_eq!(before_drive, before);
    resumed.start_mail();
    resumed
        .transport
        .observer
        .wait_for(
            |events| {
                events
                    .iter()
                    .any(|event| matches!(event, WireEvent::MailSubmit(id) if *id == item))
                    && events
                        .iter()
                        .any(|event| matches!(event, WireEvent::GenericSubmit(id) if *id == item))
            },
            "reopened engine did not submit existing remote custody",
        )
        .await?;
    let canonical = resumed.query(vec![item]).await?;
    assert!(
        matches!(canonical.statuses.as_slice(), [status] if status.item == item && matches!(&status.state, ItemStatusState::Known { digest: actual, status: fleet_sync_protocol::DeliveryStatus::Sent { .. }, .. } if actual == &digest))
    );
    // KEEP_RESULT=false removes this original operation and its steps only on
    // successful completion; absence alone would not prove a remote submission.
    tokio::time::timeout(BOUND, async {
        loop {
            let (record, steps) = resumed.mail.operation(operation).await?;
            if record.is_none() {
                assert!(steps.is_empty());
                return Ok::<_, TestError>(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(|_| io::Error::other("original SDK submit operation did not settle after reopen"))??;
    let rows = resumed.mail.rows().await?;
    let row = rows
        .iter()
        .find(|row| row.category == "outbox" && row.identity == item.as_uuid().to_string())
        .ok_or("resumed sender header missing")?;
    let header: serde_json::Value = serde_json::from_str(
        row.content
            .as_deref()
            .ok_or("resumed sender header content missing")?,
    )?;
    assert_eq!(header["value"]["status"]["kind"], "sent");
    assert_eq!(header["value"]["digest"], digest.as_str());
    assert_eq!(row.payload_bytes.as_deref(), Some("0"));
    assert!(
        resumed.mail.state().await?.is_none(),
        "a new receive admission masked SDK recovery"
    );
    assert!(
        !resumed
            .transport
            .observer
            .events()
            .iter()
            .any(|event| matches!(event, WireEvent::Fetch { .. })),
        "a fresh receive cycle masked sender recovery"
    );
    assert!(!*resumed.welcomed.borrow());
    assert_eq!(
        resumed.connections(),
        1,
        "only the fresh existing-session pump may connect"
    );
    resumed.release_welcome()?;
    resumed.close().await
}
