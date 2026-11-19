use super::*;
use crate::{
    WorkspaceCaller,
    organization_authority::machine_login::{EnrollmentGeneration, MachineRow, StoredEnrollment},
};
use workspace_model::Caller;

const NOW_MS: i64 = 1_800_000_000_123;
fn id(suffix: u64) -> String {
    format!("018f1111-2222-7000-8000-{suffix:012x}")
}
fn attachment() -> MachineConnectionAttachment {
    MachineConnectionAttachment {
        organization_id: id(1).try_into().unwrap(),
        machine_id: id(3).try_into().unwrap(),
        enrollment_id: id(4).try_into().unwrap(),
        client_id: id(5).try_into().unwrap(),
        effective_expires_at: UnixSeconds(NOW_MS / 1000 + 600),
    }
}
fn facts() -> EnrollmentFacts {
    let attachment = attachment();
    let enrollment = StoredEnrollment {
        id: attachment.enrollment_id.clone(),
        machine_id: attachment.machine_id.clone(),
        request_id: id(6).try_into().unwrap(),
        approved_by: id(7).try_into().unwrap(),
        label: "test machine".to_owned().try_into().unwrap(),
        allowed_workspaces: Vec::new().try_into().unwrap(),
        base_generation: EnrollmentGeneration::try_from(0).unwrap(),
        approved_at_ms: NOW_MS - 1000,
        code_expires_at: UnixSeconds(NOW_MS / 1000 + 100),
        state: StoredEnrollmentState::Enrolled {
            client_id: attachment.client_id.clone(),
            last_grant_at: UnixSeconds(NOW_MS / 1000),
        },
        ended_at_ms: None,
    };
    EnrollmentFacts {
        organization_id: attachment.organization_id.clone(),
        organization_has_enrollment: true,
        prior_approval: None,
        enrollment: Some(enrollment.clone()),
        machine: Some(MachineRow {
            machine_id: attachment.machine_id,
            label: "test machine".to_owned().try_into().unwrap(),
            allowed_workspaces: vec![],
            generation: EnrollmentGeneration::try_from(1).unwrap(),
            state: MachineState::Enrolled,
        }),
        active: Some(enrollment),
        superseded_client: None,
        missing_workspaces: vec![],
    }
}
fn admit(attachment: &MachineConnectionAttachment, facts: &EnrollmentFacts) -> Admission {
    validate_live(attachment, &attachment.organization_id, facts, NOW_MS)
}

#[test]
fn caller_uses_the_admitted_machine_and_refuses_a_different_machine() {
    let provenance = attachment();
    let connection = admit(&provenance, &facts()).unwrap().unwrap();
    assert_eq!(connection.machine_id().as_str(), id(3));
    let caller = |suffix| Caller::Person {
        person_id: id(7).parse().unwrap(),
        machine_id: id(suffix).parse().unwrap(),
    };
    // Act / Assert: only the machine_id may bind a semantic request caller.
    assert!(WorkspaceCaller::new(connection.clone(), caller(3)).is_ok());
    assert!(matches!(
        WorkspaceCaller::new(connection, caller(2)),
        Err(workspace_model::RefusalReason::MachineRevoked)
    ));
}

#[test]
fn live_admission_requires_matching_active_machine_enrollment_and_client() {
    let provenance = attachment();
    let baseline = facts();
    assert!(admit(&provenance, &baseline).unwrap().is_ok());
    let mut absent = baseline.clone();
    absent.machine = None;
    assert!(matches!(
        admit(&provenance, &absent).unwrap(),
        Err(MachineConnectionRefusal::UnknownMachine {})
    ));
    let mut revoked = baseline.clone();
    revoked.machine.as_mut().unwrap().state = MachineState::Revoked;
    assert!(matches!(
        admit(&provenance, &revoked).unwrap(),
        Err(MachineConnectionRefusal::EnrollmentNotActive {})
    ));
    let mut superseded = baseline.clone();
    superseded.active.as_mut().unwrap().id = id(9).try_into().unwrap();
    assert!(matches!(
        admit(&provenance, &superseded).unwrap(),
        Err(MachineConnectionRefusal::Superseded {})
    ));
    let mut wrong_client = provenance.clone();
    wrong_client.client_id = id(9).try_into().unwrap();
    assert!(matches!(
        admit(&wrong_client, &baseline).unwrap(),
        Err(MachineConnectionRefusal::WrongClient {})
    ));
    let mut wrong_row = provenance.clone();
    wrong_row.machine_id = id(9).parse().unwrap();
    assert!(matches!(
        admit(&wrong_row, &baseline).unwrap(),
        Err(MachineConnectionRefusal::EnrollmentNotActive {})
    ));
    let mut wrong_organization = baseline;
    wrong_organization.organization_id = id(9).try_into().unwrap();
    assert!(matches!(
        admit(&provenance, &wrong_organization).unwrap(),
        Err(MachineConnectionRefusal::MachineInAnotherOrganization {})
    ));
}

#[test]
fn original_deadline_has_no_leeway_and_never_renews_during_admission() {
    let provenance = attachment();
    let facts = facts();
    let deadline_ms = provenance.effective_expires_at.0 * 1000;
    assert!(
        validate_live(
            &provenance,
            &provenance.organization_id,
            &facts,
            deadline_ms - 1
        )
        .unwrap()
        .is_ok()
    );
    for now_ms in [deadline_ms, deadline_ms + 1] {
        assert!(matches!(
            validate_live(&provenance, &provenance.organization_id, &facts, now_ms).unwrap(),
            Err(MachineConnectionRefusal::EnrollmentNotActive {})
        ));
    }
    let connection = admit(&provenance, &facts).unwrap().unwrap();
    assert_eq!(
        connection.effective_expires_at(),
        provenance.effective_expires_at
    );
    assert_eq!(connection.attachment(), provenance);
}

#[test]
fn attachment_is_closed_and_restoration_rejects_invalid_identity_and_time() {
    let provenance = attachment();
    let encoded = serde_json::to_value(&provenance).unwrap();
    assert_eq!(encoded.as_object().unwrap().len(), 5);
    let mut extra = encoded.clone();
    extra["permissions"] = serde_json::json!([]);
    assert!(serde_json::from_value::<MachineConnectionAttachment>(extra).is_err());
    let mut missing = encoded;
    missing.as_object_mut().unwrap().remove("enrollmentId");
    assert!(serde_json::from_value::<MachineConnectionAttachment>(missing).is_err());
    let mut non_v7 = serde_json::to_value(&provenance).unwrap();
    non_v7["machineId"] = serde_json::json!("018f1111-2222-4000-8000-000000000003");
    assert!(serde_json::from_value::<MachineConnectionAttachment>(non_v7).is_err());
    for expiry in [-1, i64::MAX] {
        let mut invalid = provenance.clone();
        invalid.effective_expires_at = UnixSeconds(expiry);
        assert!(matches!(
            admit(&invalid, &facts()),
            Err(StoreFailure::Corrupt { .. })
        ));
    }
}
