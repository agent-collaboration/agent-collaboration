//! A selected machine-token connection cannot silently become an enrollment connection.
use super::CodexHarnessAdmission;
use collaboration_contracts::{
    HarnessLaunchFailure, HarnessLaunchFuture, HarnessLocalSetupRefusal,
    HarnessRegistrationOutcome, HarnessRestoreAdmission, HarnessSessionLaunchPort,
    HarnessStartAdmission,
};
use collaboration_protocol::OperationId;
use message_board::SessionRef;
use serde_json::json;
use std::sync::Arc;

struct EnrollmentResponseLaunch;
impl HarnessSessionLaunchPort for EnrollmentResponseLaunch {
    fn prepare_start<'a>(
        &'a self,
        _: &'a OperationId,
        _: &'a collaboration_contracts::HarnessDeclaredLaunchInput,
    ) -> HarnessLaunchFuture<'a, HarnessStartAdmission> {
        Box::pin(async { Ok(HarnessStartAdmission::AccessEnrollment) })
    }
    fn binding_for_restore<'a>(
        &'a self,
        _: &'a SessionRef,
    ) -> HarnessLaunchFuture<'a, HarnessRestoreAdmission> {
        Box::pin(async { Ok(HarnessRestoreAdmission::AccessEnrollment) })
    }
    fn mark_dispatched<'a>(&'a self, _: &'a OperationId) -> HarnessLaunchFuture<'a, ()> {
        unavailable()
    }
    fn record_created<'a>(
        &'a self,
        _: &'a OperationId,
        _: &'a SessionRef,
    ) -> HarnessLaunchFuture<'a, ()> {
        unavailable()
    }
    fn creation_refused<'a>(&'a self, _: &'a OperationId) -> HarnessLaunchFuture<'a, ()> {
        unavailable()
    }
    fn complete_setup<'a>(
        &'a self,
        _: &'a OperationId,
    ) -> HarnessLaunchFuture<'a, HarnessRegistrationOutcome> {
        unavailable()
    }
    fn refuse_setup<'a>(
        &'a self,
        _: &'a OperationId,
        _: HarnessLocalSetupRefusal,
    ) -> HarnessLaunchFuture<'a, ()> {
        unavailable()
    }
}
fn unavailable<'a, TOutcome: Send + 'a>() -> HarnessLaunchFuture<'a, TOutcome> {
    Box::pin(async { Err(HarnessLaunchFailure::Unavailable) })
}

#[tokio::test]
async fn native_admission_gate_start_keeps_explicit_enrollment_but_refuses_a_mode_downgrade() {
    let endpoint =
        json!({"serviceId":"019f0000-0000-7000-8000-000000000003","endpointId":"codex-local"});
    let actor = json!({"endpoint":endpoint,"sessionId":"permission-route-owner"});
    let params = json!({"cwd":"/fixture/work","mcpServers":[],"_meta":{"codexRouter":{
        "projectId":"019f0000-0000-7000-8000-000000000004","model":"fixture-model","effort":"low","access":"workspace-write",
        "createdBy":actor,"approver":actor
    }}});
    let operation = "019f0000-0000-7000-8000-000000000020"
        .to_owned()
        .try_into()
        .unwrap();
    let mut catalog = crate::AcpSchemaCatalog::load().unwrap();
    let enrollment = CodexHarnessAdmission::AccessEnrollment
        .prepare_start(&mut catalog, &params, &operation)
        .await;
    assert!(matches!(
        enrollment,
        Ok(crate::HarnessSessionSetup::AccessEnrollment)
    ));
    let selected = CodexHarnessAdmission::MachineTokens {
        endpoint: serde_json::from_value(endpoint).unwrap(),
        port: Arc::new(EnrollmentResponseLaunch),
    };

    let outcome = selected
        .prepare_start(&mut catalog, &params, &operation)
        .await;

    assert!(matches!(
        outcome,
        Err(crate::SessionSetupError::Unavailable)
    ));
}

#[tokio::test]
async fn native_admission_gate_restore_keeps_explicit_enrollment_but_refuses_a_mode_downgrade() {
    let params = json!({"sessionId":"previously-created-conversation"});
    let enrollment = CodexHarnessAdmission::AccessEnrollment
        .restore(&params)
        .await;
    assert!(matches!(
        enrollment,
        Ok(crate::HarnessSessionSetup::AccessEnrollment)
    ));
    let selected = CodexHarnessAdmission::MachineTokens {
        endpoint: serde_json::from_value(json!({
            "serviceId":"019f0000-0000-7000-8000-000000000003","endpointId":"codex-local"
        }))
        .unwrap(),
        port: Arc::new(EnrollmentResponseLaunch),
    };

    let outcome = selected.restore(&params).await;

    assert!(matches!(
        outcome,
        Err(crate::SessionSetupError::Unavailable)
    ));
}
