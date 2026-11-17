//! A Router installation resolves to the machine it enrolled as, whatever the
//! UUID version of its installation id.
use super::*;
use central_storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, StatementBatch,
};

#[tokio::test]
async fn an_installation_id_of_any_rfc_version_resolves_to_its_enrolled_machine() {
    let f = Fixture::new().await;
    let runtime = f.host.central(&f.caller.organization_id).await.unwrap();
    let live_v4 = "4f6a2c1e-8d3b-4a7e-9c2d-1b5e7f9a0c3d";
    let moved = runtime
        .statement_host()
        .run_batch(StatementBatch {
            statements: vec![GuardedStatement {
                statement: Statement {
                    sql: "UPDATE machines SET installation_id=? WHERE machine_id=?",
                    args: vec![text(live_v4), text(f.caller.caller.machine_id())],
                },
                expect: RowExpectation::Exactly(1),
            }],
            alarm: AlarmUpdate::Unchanged,
        })
        .await
        .unwrap();
    assert!(matches!(moved, BatchOutcome::Applied { .. }));

    let found = runtime
        .machine_for_installation(&live_v4.parse().unwrap())
        .await
        .unwrap();
    let unknown = runtime
        .machine_for_installation(&"5f6a2c1e-8d3b-4a7e-9c2d-1b5e7f9a0c3d".parse().unwrap())
        .await
        .unwrap();

    assert_eq!(found.as_ref(), Some(f.caller.caller.machine_id()));
    assert_eq!(unknown, None);
    f.host.close().await;
}
