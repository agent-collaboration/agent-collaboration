use super::{CORE_TYPED_CANONICAL_INPUTS, ReadCostObservation};

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Every normalized canonical input row must remain in the real trace counter."
)]
async fn typed_canonical_heads_and_children_remain_visible_in_the_core_row_budget()
-> Result<(), Box<dyn std::error::Error>> {
    // Arrange the six actual owning SQL statements and a literal three returned rows each.
    let observation = ReadCostObservation::default();
    // Act through the same SQLx trace boundary consumed by the permanent scaling proofs.
    observation.observe(async {
        for statement in CORE_TYPED_CANONICAL_INPUTS {
            tracing::event!(target: "sqlx::query", tracing::Level::TRACE,
                db.statement = *statement, rows_returned = 3_u64, summary = "typed canonical input");
        }
    }).await;
    // Assert four head statements plus two ordered-child statements: 6 * 3 rows.
    assert_eq!(observation.cost().core_canonical_input_bodies, 18);
    assert_eq!(observation.cost().statements, 6);
    Ok(())
}
