use super::support::*;
use central_storage::StoreFailure;
use collaboration_central_core::WorkspaceCaller;
use collaboration_central_core::identity_contract::ProvisionOrganization;
use collaboration_central_core::organization_authority::OrganizationProvisioningFailure;
use collaboration_protocol::central_mail::{
    CentralMailAnswer, CentralMailOperation, CentralMailRequest, CentralMailResult,
};
use router_clock::RuntimeClock;
#[expect(
    clippy::panic_in_result_fn,
    reason = "A cancelled real commit followed by a failed alarm reconciliation must preserve retry classification and exact stored effects."
)]
#[tokio::test]
async fn known_provisioning_commit_reconciles_on_same_host_and_failed_repair_is_uncertain() -> Result
{
    let f = Fixture::new().await?;
    f.seed_machine().await?;
    let target = workspace_model::SessionAddress {
        machine_id: MACHINE.to_owned().try_into()?,
        endpoint: "codex-local".to_owned().try_into()?,
        session_id: "provision-repair".to_owned().try_into()?,
    };
    let caller = WorkspaceCaller {
        organization_id: org(A)?,
        caller: workspace_model::Caller::Session(target.clone()),
        credential_hash: "signin-fixture-hash".to_owned(),
    };
    let due = chrono::DateTime::from_timestamp(
        (f.clock.utc_now().timestamp().div_euclid(300) + 1) * 300,
        0,
    )
    .ok_or("wake grid mark exceeds timestamp range")?;
    let wake = f
        .host
        .request_central(
            &caller,
            CentralMailRequest {
                request_id: signed::fresh_request_id(signed::NOW_SECONDS)?.try_into()?,
                operation: CentralMailOperation::SetWake {
                    target,
                    due_at: due,
                    text: "before stamp".to_owned().try_into()?,
                },
            },
        )
        .await?;
    assert!(matches!(
        wake,
        CentralMailResult::Done(CentralMailAnswer::WakeSet { .. })
    ));
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
