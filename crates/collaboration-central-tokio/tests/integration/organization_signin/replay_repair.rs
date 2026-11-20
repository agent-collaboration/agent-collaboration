use super::support::*;
use central_storage::StoreFailure;
use collaboration_central_core::identity_contract::ProvisionOrganization;
use fleet_contracts::OrganizationProvisioningFailure;
#[expect(
    clippy::panic_in_result_fn,
    reason = "A cancelled real commit followed by a failed alarm reconciliation must preserve retry classification and exact stored effects."
)]
#[tokio::test]
async fn known_provisioning_commit_reconciles_on_same_host_and_failed_repair_is_uncertain() -> Result
{
    let f = Fixture::new().await?;
    f.seed_machine().await?;
    // Pending durable work predates provisioning; C2 correctly cannot authenticate an unstamped org.
    let (_wake_id, due) = f.seed_pending_wake("provision-repair").await?;
    let runtime = f.runtime(A).await?;
    assert_eq!(runtime.statement_host().alarm(), None);
    let mut held = runtime.statement_host().hold_alarm_publication();
    let statement = signed::stamp(A, signed::NOW_SECONDS)?;
    let request = ProvisionOrganization {
        organization_id: org(A)?,
        organization_statement: statement.clone().try_into()?,
    };
    let task_runtime = runtime.clone();
    let task_request = request.clone();
    let pending =
        tokio::spawn(async move { task_runtime.provision_organization(&task_request).await });
    held.until_held().await;
    let committed = f.snapshot().await?;
    assert_eq!((committed.0.len(), committed.1.len()), (1, 1));
    assert_eq!(runtime.statement_host().alarm(), None);
    pending.abort();
    assert!(pending.await.unwrap_err().is_cancelled());
    drop(held);
    runtime
        .statement_host()
        .fail_next_alarm_batch(StoreFailure::Unavailable);
    assert_eq!(
        runtime.provision_organization(&request).await,
        Err(OrganizationProvisioningFailure::OutcomeUnknown)
    );
    assert_eq!(runtime.statement_host().alarm(), None);
    assert!(
        f.snapshot().await? == committed,
        "uncertain repair must not duplicate the known stamp/record"
    );
    assert_eq!(f.provision(A, &statement).await?.status, 200);
    assert_eq!(
        runtime.statement_host().alarm().map(|at| at.unix_millis()),
        Some(due.timestamp_millis())
    );
    assert!(
        f.snapshot().await? == committed,
        "same-host reconciliation returns the durable first outcome"
    );
    f.close().await
}
