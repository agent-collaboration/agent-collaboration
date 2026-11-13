use super::QueryResult;

#[test]
fn extend_sums_changes_and_keeps_the_last_reported_rowid() {
    let mut result = QueryResult::default();

    result.extend([
        QueryResult::new(2, 7),
        QueryResult::new(1, 9),
        QueryResult::new(0, 0),
    ]);

    assert_eq!(result.rows_affected(), 3);
    assert_eq!(result.last_insert_rowid(), 9);
}
