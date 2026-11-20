//! Shared real-route setup inputs and opaque owning-row comparisons.
pub(super) use super::super::support::*;
use central_storage::SqlValue;
use serde_json::{Value, json};

pub(super) fn body(operation: &str, name: &str, description: &str) -> Value {
    json!({"organizationId":A,"operationId":operation,"name":name,"description":description})
}
pub(super) fn intent(operation: &str, name: &str, description: &str) -> Value {
    json!({"v":1,"operation":"CreateWorkspace","organization":A,"target":A,"operationId":operation,"params":{"name":name,"description":description}})
}
pub(super) async fn create(
    f: &Fixture,
    person: &signed::Person,
    operation: &str,
    name: &str,
    description: &str,
) -> Result<Reply> {
    f.person(
        signed::CREATE,
        &body(operation, name, description),
        person,
        &intent(operation, name, description),
        &signed::fresh_request_id(f.clock_time())?,
    )
    .await
}
pub(super) async fn durable_effects(f: &Fixture) -> Result<Vec<Vec<Vec<SqlValue>>>> {
    let mut rows = vec![];
    for sql in [
        "SELECT * FROM directory_workspaces ORDER BY workspace_id",
        "SELECT * FROM runtime_request_log ORDER BY request_id",
        "SELECT * FROM signin_change_records ORDER BY id",
        "SELECT * FROM access_actor_identities ORDER BY actor_identity_id",
    ] {
        rows.push(f.rows(A, sql, vec![]).await?);
    }
    Ok(rows)
}
pub(super) async fn all_effects(f: &Fixture) -> Result<Vec<Vec<Vec<SqlValue>>>> {
    let mut rows = durable_effects(f).await?;
    rows.push(
        f.rows(A, "SELECT * FROM admin_used_requests ORDER BY id", vec![])
            .await?,
    );
    for sql in [
        "SELECT * FROM signin_authorization_floors ORDER BY id",
        "SELECT * FROM signin_person_authorization_floors ORDER BY id",
    ] {
        rows.push(f.rows(A, sql, vec![]).await?);
    }
    Ok(rows)
}
pub(super) async fn machine_rows(f: &Fixture) -> Result<Vec<Vec<Vec<SqlValue>>>> {
    let mut rows = vec![];
    for sql in [
        "SELECT * FROM login_machines ORDER BY machine_id",
        "SELECT name FROM sqlite_schema WHERE type='table' AND name='machine_credentials'",
        "SELECT * FROM login_machine_approvals ORDER BY approval_id",
    ] {
        rows.push(f.rows(A, sql, vec![]).await?);
    }
    Ok(rows)
}
pub(super) async fn caller(f: &Fixture) -> Result<fleet_contracts::WorkspaceCaller> {
    f.machine_caller("setup-machine-session").await
}
