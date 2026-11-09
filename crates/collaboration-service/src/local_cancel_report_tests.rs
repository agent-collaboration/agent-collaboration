//! The internal local producer must journal remote report debt in its outcome transaction.
use super::*;
use durable_steps::{AtomicStore, JournalScope, StoreError, Task};
use fleet_mail_delivery::{Inbox, ReportOutcome, delivery_request_key};

type ReportTask = ReportOutcome<
    ProjectionStore,
    <durable_steps::turso_store::TursoScope<'static, ProjectionHost> as durable_steps::storage::TransactionScope>::Database,
    RouterDeliveryTypes,
    crate::MachineSenderBinding,
>;

#[tokio::test]
async fn local_cancel_journals_the_report_for_an_actual_relayed_inbox_original() {
    // Only the receiver is in this lower-layer check; the real two-Host case
    // separately proves canonical control admission and central acknowledgement.
    let runtime = Runtime::new().await;
    let mail = runtime.local_mail_runtime();
    let draft = runtime.fixture.draft();
    let original = runtime.fixture.item(&draft).id();
    runtime.fixture.receive(draft.clone()).await.unwrap();
    let cancel = ItemId::new(uuid::Uuid::now_v7()).unwrap();
    mail.cancel_local(
        cancel_for(&runtime, &draft, cancel),
        requester("remote report producer"),
    )
    .await
    .unwrap();
    assert_eq!(
        runtime.fixture.local_outcome(original).await,
        Some(RecipientOutcome::Prevented { cancel })
    );
    assert_eq!(
        runtime.fixture.report(original).await,
        Some(RecipientOutcome::Prevented { cancel })
    );
    let config = runtime.fixture.host.config.local;
    let binding = runtime.fixture.host.binding.clone();
    runtime
        .fixture
        .store
        .transaction(move |scope| {
            Box::pin(async move {
                let report = Inbox::<_, RouterDeliveryTypes, _>::new(
                    scope,
                    config.owner,
                    config.limits,
                    binding.as_ref(),
                )
                .pending_report(original)
                .await
                .map_err(StoreError::backend)?
                .ok_or_else(|| StoreError::backend("prevention report was not retained"))?;
                let requests = scope.running_request_keys(ReportTask::KIND).await?;
                assert_eq!(
                    requests,
                    vec![
                        delivery_request_key(&(original, report.report_seq))
                            .map_err(StoreError::backend)?
                    ],
                    "owed original report has no durable report operation"
                );
                Ok(())
            })
        })
        .await
        .unwrap();
    assert_eq!(runtime.client.calls(original), 0);
    drop(mail);
    runtime.close().await;
}

#[tokio::test]
async fn a_final_local_push_promotion_reports_the_preserved_outcome_once_across_replay() {
    // Arrange prevention through the real local producer, before any Fleet hop.
    let runtime = Runtime::new().await;
    let mail = runtime.local_mail_runtime();
    let draft = runtime.fixture.draft();
    let original = runtime.fixture.item(&draft).id();
    let cancel = ItemId::new(uuid::Uuid::now_v7()).unwrap();
    mail.cancel_local(
        cancel_for(&runtime, &draft, cancel),
        requester("local prevention before Push replay"),
    )
    .await
    .unwrap();
    mail.send_local(draft.clone(), "already prevented local original".into())
        .await
        .unwrap();
    assert_eq!(runtime.fixture.report(original).await, None);

    // Act twice through real accept_push. The first promotion owes a durable
    // report even if its receipt is lost; a second replay must not emit again.
    for replay in 0..2 {
        let receipt = runtime
            .fixture
            .push_item(runtime.fixture.item(&draft))
            .await
            .unwrap();
        assert!(
            matches!(
                receipt,
                fleet_protocol::PushResult::Replayed {
                    status: fleet_protocol::DeliveryStatus::Cancelled,
                    cancel: None
                }
            ),
            "replay {replay}: {receipt:?}"
        );
        assert_eq!(
            runtime.fixture.report(original).await,
            Some(RecipientOutcome::Prevented { cancel })
        );
        assert_eq!(
            runtime.fixture.local_outcome(original).await,
            Some(RecipientOutcome::Prevented { cancel })
        );
        let reports = runtime
            .fixture
            .store
            .transaction(|scope| {
                Box::pin(async move { scope.running_request_keys(ReportTask::KIND).await })
            })
            .await
            .unwrap();
        // Local prevention is sequence1; promotion is the single sequence2
        // Report. The second Push cannot add sequence3 or a second operation.
        assert_eq!(
            reports,
            vec![delivery_request_key(&(original, fleet_protocol::ReportSeq::new(2))).unwrap()],
            "replay {replay} duplicated the preserved Report"
        );
    }
    assert_eq!(runtime.client.calls(original), 0);
    drop(mail);
    runtime.close().await;
}
