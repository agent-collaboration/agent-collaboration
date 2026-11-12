use super::{OwnedStoreCall, StoreAnswer};
use collaboration_central_core::storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, SqlValue, Statement,
    StatementBatch, StoreCall, StoreFailure, StoreReply,
};

#[test]
fn a_store_call_crosses_the_object_boundary_unchanged() {
    let call = StoreCall::Batch(StatementBatch {
        statements: vec![GuardedStatement {
            statement: Statement {
                sql: "INSERT INTO t (a, b, c, d, e) VALUES (?, ?, ?, ?, ?)",
                args: vec![
                    SqlValue::Null,
                    SqlValue::Integer(-7),
                    SqlValue::Real(1.5),
                    SqlValue::Text("x".into()),
                    SqlValue::Blob(vec![0, 255]),
                ],
            },
            expect: RowExpectation::Exactly(1),
        }],
        alarm: AlarmUpdate::SetFromQuery(Statement {
            sql: "SELECT 1",
            args: Vec::new(),
        }),
    });
    let owned = OwnedStoreCall::from(call);
    let decoded: OwnedStoreCall =
        serde_json::from_slice(&serde_json::to_vec(&owned).unwrap()).unwrap();
    assert_eq!(decoded, owned);
}

#[test]
fn a_store_answer_keeps_its_variant() {
    for answer in [
        StoreAnswer::Ok(StoreReply::Batch(BatchOutcome::GuardFailed {
            index: 1,
            rows_affected: 0,
        })),
        StoreAnswer::Err(StoreFailure::Unavailable),
    ] {
        let decoded: StoreAnswer =
            serde_json::from_slice(&serde_json::to_vec(&answer).unwrap()).unwrap();
        assert_eq!(decoded, answer);
    }
}
