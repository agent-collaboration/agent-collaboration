//! Pure construction of statement arguments and unguarded row-count expectations.
use central_storage::{GuardedStatement, RowExpectation, SqlValue, Statement};

pub fn text(value: impl ToString) -> SqlValue {
    SqlValue::Text(value.to_string())
}

pub fn any(sql: &'static str, args: Vec<SqlValue>) -> GuardedStatement {
    GuardedStatement {
        statement: Statement { sql, args },
        expect: RowExpectation::Any,
    }
}
