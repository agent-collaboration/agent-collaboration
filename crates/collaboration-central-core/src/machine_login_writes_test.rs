use super::*;
use crate::organization_authority::machine_login::EnrollmentGeneration;
use fleet_identity_protocol::UnixSeconds;

fn machine() -> MachineRow {
    MachineRow {
        id: values::private_id("019a3dce-2000-7000-8000-000000000002", "machine_id").unwrap(),
        installation: "019a3dce-2000-7000-8000-000000000003"
            .to_owned()
            .try_into()
            .unwrap(),
        label: "Current label".to_owned().try_into().unwrap(),
        allowed_workspaces: vec![],
        generation: EnrollmentGeneration::try_from(3).unwrap(),
        state: MachineState::Enrolled,
    }
}
fn enrollment() -> StoredEnrollment {
    StoredEnrollment {
        id: "019a3dce-2000-7000-8000-000000000001"
            .to_owned()
            .try_into()
            .unwrap(),
        machine_id: machine().id,
        installation: machine().installation,
        request_id: "019a3dce-2000-7000-8000-000000000004"
            .to_owned()
            .try_into()
            .unwrap(),
        approved_by: "019a3dce-2000-7000-8000-000000000005"
            .to_owned()
            .try_into()
            .unwrap(),
        label: "Approval label".to_owned().try_into().unwrap(),
        allowed_workspaces: vec![].try_into().unwrap(),
        base_generation: EnrollmentGeneration::try_from(3).unwrap(),
        approved_at_ms: 1_900_000_000_000,
        code_expires_at: UnixSeconds(1_900_000_600),
        state: StoredEnrollmentState::Approved,
        ended_at_ms: None,
    }
}
fn facts() -> EnrollmentFacts {
    EnrollmentFacts {
        organization_id: "019a3dce-2000-7000-8000-000000000006"
            .to_owned()
            .try_into()
            .unwrap(),
        organization_has_enrollment: true,
        prior_approval: None,
        enrollment: Some(enrollment()),
        machine: Some(machine()),
        active: None,
        superseded_client: None,
        missing_workspaces: vec![],
    }
}

#[test]
fn activation_guards_the_same_applying_time_and_never_updates_approval_metadata() {
    let before = enrollment();
    let next = StoredEnrollment {
        state: StoredEnrollmentState::Enrolled {
            client_id: before.id.as_str().to_owned().try_into().unwrap(),
            last_grant_at: UnixSeconds(1_900_000_001),
        },
        ..before
    };
    let plan = statements(
        &facts(),
        &[EnrollmentChange::ReplaceEnrollment(next)],
        1_900_000_001_234,
    )
    .unwrap();
    assert_eq!(plan.len(), 3);
    assert!(plan[0].statement.sql.contains("enrollment_generation=?"));
    assert!(
        plan[1]
            .statement
            .sql
            .contains("unixepoch(code_expires_at)*1000>?")
    );
    assert_eq!(
        plan[1].statement.args.last(),
        Some(&integer(1_900_000_001_234, "now").unwrap())
    );
    assert!(plan[2].statement.sql.starts_with("UPDATE machine_approvals SET state=?,end_reason=?,machine_client_id=?,last_grant_at=?,ended_at=?,updated_at=? WHERE"));
    assert!(!plan[2].statement.sql.contains("SET label="));
    assert!(
        plan.iter()
            .all(|statement| statement.expect == central_storage::RowExpectation::Exactly(1))
    );
}

#[test]
fn changing_any_first_approval_reply_field_is_not_a_lifecycle_write() {
    let mut changed = enrollment();
    changed.label = "Replacement label".to_owned().try_into().unwrap();
    assert_eq!(
        replace_enrollment(&enrollment(), &changed, "2030-03-17T17:46:41.000Z"),
        Err(corrupt("machine_approvals.immutable_approval"))
    );
    let mut changed = enrollment();
    changed.allowed_workspaces = vec![
        "019a3dce-2000-7000-8000-000000000009"
            .to_owned()
            .try_into()
            .unwrap(),
    ]
    .try_into()
    .unwrap();
    assert_eq!(
        replace_enrollment(&enrollment(), &changed, "2030-03-17T17:46:41.000Z"),
        Err(corrupt("machine_approvals.immutable_approval"))
    );
}

#[test]
fn new_machine_keys_and_generation_are_never_rewritten_or_wrapped() {
    let before = machine();
    let mut rekeyed = before.clone();
    rekeyed.id = values::private_id("019a3dce-2000-7000-8000-000000000009", "machine_id").unwrap();
    assert_eq!(
        put_machine(Some(&before), &rekeyed, "2030-03-17T17:46:41.000Z"),
        Err(corrupt("machines.machine_id"))
    );
    let mut skipped = before.clone();
    skipped.generation = EnrollmentGeneration::try_from(5).unwrap();
    assert_eq!(
        put_machine(Some(&before), &skipped, "2030-03-17T17:46:41.000Z"),
        Err(corrupt("machines.enrollment_generation"))
    );
    let mut new = before;
    new.generation = EnrollmentGeneration::default();
    new.state = MachineState::Revoked;
    assert!(
        put_machine(None, &new, "2030-03-17T17:46:41.000Z")
            .unwrap()
            .statement
            .sql
            .starts_with("INSERT INTO machines")
    );
    assert!(
        statements(&facts(), &[], 1_900_000_000_000)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn terminal_writes_clear_grant_time_and_preserve_the_client_for_disable() {
    let before = enrollment();
    let ended = StoredEnrollment {
        state: StoredEnrollmentState::Revoked {
            client_id: before.id.as_str().to_owned().try_into().unwrap(),
            reason: EnrollmentRevocationReason::Owner {},
        },
        ended_at_ms: Some(1_900_000_001_000),
        ..before
    };
    let encoded = state(&ended).unwrap();
    assert_eq!((encoded.state, encoded.reason), ("ended", Some("owner")));
    assert!(encoded.last_grant.is_none());
    assert!(encoded.client.as_deref() == Some(ended.id.as_str()));
    assert_eq!(
        encoded.ended_at.as_deref(),
        Some("2030-03-17T17:46:41.000Z")
    );
}
