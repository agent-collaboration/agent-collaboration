use super::*;
use central_storage::{BatchOutcome, ReadOutcome, SqlValue, StatementBatch};
use std::sync::Mutex;

struct ReadSequence(Mutex<Vec<(&'static str, ReadOutcome)>>);
impl CentralStore for ReadSequence {
    async fn published_alarm(
        &self,
    ) -> Result<Option<central_engine_model::EngineTime>, central_storage::StoreFailure> {
        panic!("this test double never reads the physical alarm")
    }
    async fn read(&self, statement: Statement) -> Result<ReadOutcome, StoreFailure> {
        let (sql, result) = self.0.lock().unwrap().remove(0);
        assert_eq!(statement.sql, sql);
        Ok(result)
    }
    async fn run_batch(&self, _: StatementBatch) -> Result<BatchOutcome, StoreFailure> {
        panic!("reading machine-login facts must never write")
    }
}
fn text(value: &str) -> SqlValue {
    SqlValue::Text(value.to_owned())
}
fn row(fields: Vec<(&str, SqlValue)>) -> ReadOutcome {
    ReadOutcome {
        columns: fields.iter().map(|(name, _)| (*name).to_owned()).collect(),
        rows: vec![fields.into_iter().map(|(_, value)| value).collect()],
    }
}

#[tokio::test]
async fn retained_approval_returns_before_reading_any_mutable_decision_facts() {
    // Arrange: only the retained approval can be read; any further read fails loudly.
    let stored = row(vec![
        ("approval_id", text("019a3dce-2000-7000-8000-000000000001")),
        ("request_id", text("019a3dce-2000-7000-8000-000000000004")),
        ("machine_id", text("019a3dce-2000-7000-8000-000000000003")),
        ("label", text("First approval")),
        ("allowed_workspaces", text("[]")),
        (
            "approved_by_person_id",
            text("019a3dce-2000-7000-8000-000000000005"),
        ),
        ("permission_checked", text("machine:enroll")),
        (
            "base_generation",
            SqlValue::Integer(0_i64.try_into().unwrap()),
        ),
        ("code_expires_at", text("2030-03-17T17:56:40Z")),
        ("created_at", text("2030-03-17T17:46:40Z")),
        ("updated_at", text("2030-03-17T17:46:40Z")),
        ("state", text("approved")),
        ("end_reason", SqlValue::Null),
        ("machine_client_id", SqlValue::Null),
        ("last_grant_at", SqlValue::Null),
        ("ended_at", SqlValue::Null),
    ]);
    let host = ReadSequence(Mutex::new(vec![(BY_REQUEST, stored)]));
    let query = EnrollmentQuery {
        request: Some(
            "019a3dce-2000-7000-8000-000000000004"
                .to_owned()
                .try_into()
                .unwrap(),
        ),
        machine_id: Some(
            "019a3dce-2000-7000-8000-000000000099"
                .to_owned()
                .try_into()
                .unwrap(),
        ),
        workspaces: vec![
            "019a3dce-2000-7000-8000-000000000098"
                .to_owned()
                .try_into()
                .unwrap(),
        ],
        ..EnrollmentQuery::default()
    };

    // Act: mutable machine_id/workspace choices are deliberately different.
    let found = facts(
        &host,
        "019a3dce-2000-7000-8000-000000000006"
            .to_owned()
            .try_into()
            .unwrap(),
        &query,
    )
    .await
    .unwrap();

    // Assert: the owning request row is sufficient for N03's immutable answer.
    assert_eq!(
        found.prior_approval.unwrap().label.as_str(),
        "First approval"
    );
    assert!(
        found.machine.is_none() && found.active.is_none() && found.missing_workspaces.is_empty()
    );
    assert!(host.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn history_scalar_must_be_present_and_boolean() {
    for value in [
        SqlValue::Null,
        text("1"),
        SqlValue::Integer(2_i64.try_into().unwrap()),
    ] {
        let host = ReadSequence(Mutex::new(vec![(
            "SELECT EXISTS(SELECT 1 FROM login_machine_approvals) AS has_enrollment",
            row(vec![("has_enrollment", value)]),
        )]));
        assert!(
            facts(
                &host,
                "019a3dce-2000-7000-8000-000000000006"
                    .to_owned()
                    .try_into()
                    .unwrap(),
                &EnrollmentQuery::default()
            )
            .await
            .is_err()
        );
    }
}
