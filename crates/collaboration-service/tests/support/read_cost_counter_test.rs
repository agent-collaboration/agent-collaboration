use super::ReadCostObservation;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Every normalized canonical input row must remain in the real trace counter."
)]
async fn typed_canonical_heads_and_children_remain_visible_in_the_core_row_budget()
-> Result<(), Box<dyn std::error::Error>> {
    // Arrange owning SQL inputs independently of the classifier's private list.
    // Four heads and two ordered-child reads each have a direct and batch form.
    let statements = [
        include_str!("../../../collaboration-central-core/sql/fleet/read_workspace_columns.sql"),
        include_str!(
            "../../../collaboration-central-core/sql/fleet/read_workspace_repository_refs.sql"
        ),
        include_str!("../../../collaboration-central-core/sql/fleet/read_board_columns.sql"),
        include_str!("../../../collaboration-central-core/sql/fleet/read_project_columns.sql"),
        include_str!("../../../collaboration-central-core/sql/fleet/read_adr_columns.sql"),
        include_str!("../../../collaboration-central-core/sql/fleet/read_adr_repository_files.sql"),
        include_str!("../../../collaboration-central-core/sql/fleet/read_workspace_batch.sql"),
        include_str!(
            "../../../collaboration-central-core/sql/fleet/read_workspace_repository_batch.sql"
        ),
        include_str!("../../../collaboration-central-core/sql/fleet/read_board_batch.sql"),
        include_str!("../../../collaboration-central-core/sql/fleet/read_project_batch.sql"),
        include_str!("../../../collaboration-central-core/sql/fleet/read_adr_batch.sql"),
        include_str!("../../../collaboration-central-core/sql/fleet/read_adr_file_batch.sql"),
    ];
    let observation = ReadCostObservation::default();
    // Act through the same SQLx trace boundary consumed by the permanent scaling proofs.
    observation.observe(async {
        for statement in &statements {
            tracing::event!(target: "sqlx::query", tracing::Level::TRACE,
                db.statement = *statement, rows_returned = 3_u64, summary = "typed canonical input");
        }
    }).await;
    // Assert a literal three rows for every direct/batch boundary: 12 * 3.
    assert_eq!(observation.cost().core_canonical_input_bodies, 36);
    assert_eq!(observation.cost().statements, 12);
    Ok(())
}
