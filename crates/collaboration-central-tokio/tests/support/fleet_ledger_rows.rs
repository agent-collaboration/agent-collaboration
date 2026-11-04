//! Exact physical typed ledger snapshots retain graph, evidence and ingestion metadata.
use durable_steps::{StoreError, storage::TransactionScope};
use sqlx_core::{query::query, row::Row};
const QUERIES: [&str; 3] = [
    "SELECT json_array(request_id,canonical_hash,workspace_id,board_id,actor_kind,actor_human_id,actor_service_id,actor_endpoint_id,actor_session_id,actor_router_component,envelope_request_id,fleet_position,router_time,time_basis,origin_installation,origin_machine,created_at,updated_at,decision_kind,change_kind,effects_count,effects_digest,reason_count) AS state FROM fleet_request_decisions ORDER BY request_id",
    "SELECT json_array(reason_id,request_id,parent_reason_id,ordinal,refusal_group,refusal_kind,record_kind,record_id,name,field,current_revision,retry_after_seconds,workstream_id,task_id,thread_id,requirement_kind,requirement_role,role_subject_kind,role_subject_id,authority_role,actor_kind,actor_human_id,actor_service_id,actor_endpoint_id,actor_session_id,holder_kind,holder_human_id,holder_service_id,holder_endpoint_id,holder_session_id,target_kind,target_human_id,target_service_id,target_endpoint_id,target_session_id,named_holder_kind,named_holder_human_id,named_holder_service_id,named_holder_endpoint_id,named_holder_session_id,last_seen_epoch,last_seen_position,evidence_kind,evidence_returned_count,evidence_total_count,evidence_digest,evidence_abridged,created_at,updated_at) AS state FROM fleet_decision_reasons ORDER BY request_id,parent_reason_id,ordinal",
    "SELECT json_array(row_id,reason_id,ordinal,entry_kind,task_id,assignment_kind,assignment_project_id,assignment_workstream_id,created_at,updated_at) AS state FROM fleet_decision_reason_evidence ORDER BY reason_id,ordinal",
];
#[expect(
    clippy::manual_async_fn,
    reason = "Explicit Send preserves higher-ranked borrowed snapshot scopes."
)]
pub fn in_scope<'scope, S: TransactionScope<Database = sqlx_turso::Turso>>(
    scope: &'scope mut S,
) -> impl std::future::Future<Output = Result<Vec<String>, StoreError>> + Send + 'scope {
    async move {
        let mut result = Vec::new();
        for sql in QUERIES {
            result.extend(
                query::<sqlx_turso::Turso>(sql)
                    .fetch_all(scope.executor())
                    .await
                    .map_err(StoreError::backend)?
                    .into_iter()
                    .map(|row| {
                        row.try_get::<String, _>("state")
                            .map_err(StoreError::backend)
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            );
        }
        Ok(result)
    }
}
pub async fn on_connection(
    connection: &mut sqlx_turso::TursoConnection,
) -> Result<Vec<String>, StoreError> {
    let mut result = Vec::new();
    for sql in QUERIES {
        result.extend(
            query::<sqlx_turso::Turso>(sql)
                .fetch_all(&mut *connection)
                .await
                .map_err(StoreError::backend)?
                .into_iter()
                .map(|row| {
                    row.try_get::<String, _>("state")
                        .map_err(StoreError::backend)
                })
                .collect::<Result<Vec<_>, _>>()?,
        );
    }
    Ok(result)
}
