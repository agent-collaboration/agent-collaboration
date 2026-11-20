//! Pure construction of guarded statement values, independent of any runtime.
use central_storage::{GuardedStatement, RowExpectation, SqlValue, Statement};
pub fn text(value: impl ToString) -> SqlValue {
    SqlValue::Text(value.to_string())
}
pub fn exact(sql: &'static str, args: Vec<SqlValue>) -> GuardedStatement {
    GuardedStatement {
        statement: Statement { sql, args },
        expect: RowExpectation::Exactly(1),
    }
}
pub fn any(sql: &'static str, args: Vec<SqlValue>) -> GuardedStatement {
    GuardedStatement {
        statement: Statement { sql, args },
        expect: RowExpectation::Any,
    }
}
