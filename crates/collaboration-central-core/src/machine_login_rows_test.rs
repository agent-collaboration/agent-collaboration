use super::*;
use central_storage::SafeInteger;

fn text(value: &str) -> SqlValue {
    SqlValue::Text(value.to_owned())
}
fn number(value: i64) -> SqlValue {
    SqlValue::Integer(SafeInteger::new(value).unwrap())
}
fn approval() -> ReadOutcome {
    let fields = [
        ("approval_id", text("019a3dce-2000-7000-8000-000000000001")),
        ("machine_id", text("019a3dce-2000-7000-8000-000000000003")),
        ("request_id", text("019a3dce-2000-7000-8000-000000000004")),
        (
            "approved_by_person_id",
            text("019a3dce-2000-7000-8000-000000000005"),
        ),
        ("permission_checked", text("machine:enroll")),
        ("label", text("Immutable approval label")),
        (
            "allowed_workspaces",
            text("[\"019a3dce-2000-7000-8000-000000000006\"]"),
        ),
        ("base_generation", number(3)),
        ("code_expires_at", text("2030-03-17T17:56:40Z")),
        ("created_at", text("2030-03-17T17:46:40Z")),
        ("updated_at", text("2030-03-17T17:46:40.001Z")),
        ("state", text("approved")),
        ("end_reason", SqlValue::Null),
        ("machine_client_id", SqlValue::Null),
        ("last_grant_at", SqlValue::Null),
        ("ended_at", SqlValue::Null),
    ];
    ReadOutcome {
        columns: fields.iter().map(|(name, _)| (*name).to_owned()).collect(),
        rows: vec![fields.into_iter().map(|(_, value)| value).collect()],
    }
}
fn set(rows: &mut ReadOutcome, name: &str, value: SqlValue) {
    let index = rows
        .columns
        .iter()
        .position(|column| column == name)
        .unwrap();
    rows.rows[0][index] = value;
}

#[test]
fn approval_rows_decode_by_column_name_and_preserve_the_reply_snapshot() {
    let mut rows = approval();
    rows.columns.reverse();
    rows.rows[0].reverse();
    let found = enrollment(&rows, &rows.rows[0]).unwrap();
    assert_eq!(found.label.as_str(), "Immutable approval label");
    assert_eq!(found.allowed_workspaces.as_slice().len(), 1);
    assert_eq!(found.base_generation.get(), 3);
    assert_eq!(found.approved_at_ms, 1_900_000_000_000);
    assert_eq!(found.code_expires_at.0, 1_900_000_600);
    assert_eq!(found.state, StoredEnrollmentState::Approved);
}

#[test]
fn lifecycle_columns_form_exactly_one_closed_state() {
    let mut enrolled = approval();
    set(&mut enrolled, "state", text("enrolled"));
    set(
        &mut enrolled,
        "machine_client_id",
        text("019a3dce-2000-7000-8000-000000000001"),
    );
    set(&mut enrolled, "last_grant_at", text("2030-03-17T17:46:40Z"));
    assert!(matches!(
        enrollment(&enrolled, &enrolled.rows[0]).unwrap().state,
        StoredEnrollmentState::Enrolled { .. }
    ));
    for reason in ["owner", "superseded", "idle", "lapsed"] {
        let mut ended = approval();
        set(&mut ended, "state", text("ended"));
        set(&mut ended, "end_reason", text(reason));
        set(&mut ended, "ended_at", text("2030-03-17T17:56:40Z"));
        if reason != "lapsed" {
            set(
                &mut ended,
                "machine_client_id",
                text("019a3dce-2000-7000-8000-000000000001"),
            );
        }
        assert!(enrollment(&ended, &ended.rows[0]).is_ok());
        set(&mut ended, "last_grant_at", text("2030-03-17T17:46:40Z"));
        assert_eq!(
            enrollment(&ended, &ended.rows[0]),
            Err(corrupt("login_machine_approvals.state"))
        );
    }
    for (field, value) in [
        ("state", text("revoked")),
        ("end_reason", text("owner")),
        (
            "machine_client_id",
            text("019a3dce-2000-7000-8000-000000000001"),
        ),
        ("ended_at", text("2030-03-17T17:56:40Z")),
    ] {
        let mut invalid = approval();
        set(&mut invalid, field, value);
        assert_eq!(
            enrollment(&invalid, &invalid.rows[0]),
            Err(corrupt("login_machine_approvals.state"))
        );
    }
}

#[test]
fn malformed_stored_scalars_are_refused_with_the_owning_field() {
    for (column, value, field) in [
        (
            "approval_id",
            text("00000000-0000-4000-8000-000000000001"),
            "login_machine_approvals.approval_id",
        ),
        (
            "permission_checked",
            text("machine:other"),
            "login_machine_approvals.permission_checked",
        ),
        (
            "base_generation",
            number(-1),
            "login_machine_approvals.base_generation",
        ),
        (
            "base_generation",
            text("3"),
            "login_machine_approvals.base_generation",
        ),
        (
            "label",
            text(" Padded label "),
            "login_machine_approvals.label",
        ),
        (
            "code_expires_at",
            text("2030-03-17T17:56:40.001Z"),
            "login_machine_approvals.code_expires_at",
        ),
        (
            "allowed_workspaces",
            text("[\"not-a-workspace\"]"),
            "login_machine_approvals.allowed_workspaces",
        ),
    ] {
        let mut invalid = approval();
        set(&mut invalid, column, value);
        assert_eq!(enrollment(&invalid, &invalid.rows[0]), Err(corrupt(field)));
    }
}

#[test]
fn duplicate_rows_and_legacy_machine_tags_never_become_facts() {
    let mut rows = approval();
    rows.rows.push(rows.rows[0].clone());
    assert_eq!(
        only_row(&rows, enrollment, "login_machine_approvals.request_id"),
        Err(corrupt("login_machine_approvals.request_id"))
    );
    let mut machine_rows = approval();
    machine_rows
        .columns
        .push("enrollment_generation".to_owned());
    machine_rows.rows[0].push(number(3));
    set(&mut machine_rows, "state", text("enrolled"));
    assert_eq!(
        machine(&machine_rows, &machine_rows.rows[0]).unwrap().state,
        MachineState::Enrolled
    );
    set(&mut machine_rows, "state", text("active"));
    assert_eq!(
        machine(&machine_rows, &machine_rows.rows[0]),
        Err(corrupt("login_machines.state"))
    );
}
