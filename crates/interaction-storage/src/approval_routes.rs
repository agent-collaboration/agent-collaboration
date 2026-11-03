//! Approval route payloads and their last-use retention timestamps.

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredApprovalRoute {
    pub thread_id: String,
    pub route_json: String,
}

impl StoredApprovalRoute {
    pub(crate) fn validate(&self) -> Result<(), serde_json::Error> {
        serde_json::from_str::<serde_json::Value>(&self.route_json).map(|_| ())
    }
}

pub(crate) async fn prune_expired(
    connection: &mut sqlx_turso::TursoConnection,
    cutoff_ms: i64,
    batch_size: i64,
) -> Result<u64, sqlx::Error> {
    let deleted = sqlx::query::<sqlx_turso::Turso>(
        "DELETE FROM approval_routes WHERE thread_id IN (SELECT thread_id FROM approval_routes WHERE last_used_at_ms<? ORDER BY last_used_at_ms,thread_id LIMIT ?)",
    )
    .bind(cutoff_ms)
    .bind(batch_size)
    .execute(connection)
    .await?
    .rows_affected();
    Ok(deleted)
}
