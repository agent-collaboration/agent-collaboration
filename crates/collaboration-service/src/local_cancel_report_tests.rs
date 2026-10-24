//! The internal local producer must journal remote report debt in its outcome transaction.
use super::*;
use durable_steps::{AtomicStore, JournalScope, StoreError, Task};
use fleet_sync_delivery::{Inbox, ReportOutcome, delivery_request_key};

type ReportTask = ReportOutcome<
    ProjectionStore,
    <durable_steps::turso_store::TursoScope<'static, ProjectionHost> as durable_steps::storage::TransactionScope>::Database,
    RouterDeliveryTypes,
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
    runtime
        .fixture
        .store
        .transaction(move |scope| {
            Box::pin(async move {
                let report =
                    Inbox::<_, RouterDeliveryTypes>::new(scope, config.owner, config.limits)
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
