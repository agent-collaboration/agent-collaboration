//! Pure construction of guarded statement values, independent of any runtime.
use central_storage::{GuardedStatement, RowExpectation, SqlValue, Statement};
pub(crate) fn exact(sql: &'static str, args: Vec<SqlValue>) -> GuardedStatement {
    GuardedStatement {
        statement: Statement { sql, args },
        expect: RowExpectation::Exactly(1),
    }
}
