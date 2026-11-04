//! Read canonical records through their owner within the existing snapshot scope.
use durable_steps::{
    CallFailure, CallId, CallTarget, Host, NotSentReason, StoreError, storage::TransactionScope,
};
use message_board::{Record, RecordId, RecordKind, RecordRef};
use runtime_primitives::{Clock, IdSource, SyncTime, WakeFailure, WakeScheduler};
use sqlx_core::{query::query, row::Row};

pub fn metadata_records<'scope, S: TransactionScope<Database = sqlx_turso::Turso>>(
    scope: &'scope mut S,
) -> impl std::future::Future<Output = Result<Vec<Record>, StoreError>> + Send + 'scope {
    read_keys(
        scope,
        "SELECT 'board' AS kind,board_id AS id FROM boards UNION ALL SELECT 'workspace',workspace_id FROM fleet_workspaces ORDER BY kind,id",
    )
}

pub fn records<'scope, S: TransactionScope<Database = sqlx_turso::Turso>>(
    scope: &'scope mut S,
) -> impl std::future::Future<Output = Result<Vec<Record>, StoreError>> + Send + 'scope {
    read_keys(
        scope,
        "SELECT 'board' AS kind,board_id AS id FROM boards UNION ALL SELECT 'workspace',workspace_id FROM fleet_workspaces UNION ALL SELECT 'project',project_id FROM project_directory ORDER BY kind,id",
    )
}

#[expect(
    clippy::manual_async_fn,
    reason = "Explicit Send preserves higher-ranked borrowed snapshot scopes."
)]
fn read_keys<'scope, S: TransactionScope<Database = sqlx_turso::Turso>>(
    scope: &'scope mut S,
    sql: &'static str,
) -> impl std::future::Future<Output = Result<Vec<Record>, StoreError>> + Send + 'scope {
    async move {
        let keys = query::<sqlx_turso::Turso>(sql)
            .fetch_all(scope.executor())
            .await
            .map_err(StoreError::backend)?;
        let mut records = Vec::with_capacity(keys.len());
        for row in keys {
            let kind: String = row.try_get("kind").map_err(StoreError::backend)?;
            let id: String = row.try_get("id").map_err(StoreError::backend)?;
            let reference = RecordRef {
                kind: match kind.as_str() {
                    "workspace" => RecordKind::Workspace,
                    "board" => RecordKind::Board,
                    "project" => RecordKind::Project,
                    _ => return Err(StoreError::backend("unknown fixture record kind")),
                },
                id: RecordId::try_from(id).map_err(StoreError::backend)?,
            };
            records.push(
                collaboration_central_core::read_fleet_record_in_scope(scope, &reference)
                    .await?
                    .ok_or_else(|| StoreError::backend("snapshot key has no canonical record"))?,
            );
        }
        Ok(records)
    }
}

#[expect(
    clippy::manual_async_fn,
    reason = "Explicit Send preserves higher-ranked borrowed snapshot scopes."
)]
pub fn workspace_columns<'scope, S: TransactionScope<Database = sqlx_turso::Turso>>(
    scope: &'scope mut S,
) -> impl std::future::Future<Output = Result<Vec<String>, StoreError>> + Send + 'scope {
    async move {
        let mut rows = query::<sqlx_turso::Turso>(
        "SELECT json_object('id',workspace_id,'name',name,'description',description,'state',state,'repositories',repository_count,'actorKind',actor_kind,'human',actor_human_id,'service',actor_service_id,'endpoint',actor_endpoint_id,'session',actor_session_id,'component',actor_router_component,'request',request_id,'position',fleet_position,'time',router_time,'basis',time_basis,'installation',origin_installation,'machine',origin_machine,'digest',import_digest,'created',created_at,'updated',updated_at) AS state FROM fleet_workspaces ORDER BY workspace_id"
    ).fetch_all(scope.executor()).await.map_err(StoreError::backend)?.into_iter()
        .map(|row| row.try_get("state").map_err(StoreError::backend)).collect::<Result<Vec<String>,_>>()?;
        rows.extend(query::<sqlx_turso::Turso>(
        "SELECT json_object('id',row_id,'workspace',workspace_id,'ordinal',ordinal,'kind',reference_kind,'origin',normalized_origin,'service',service_id,'directory',common_directory,'created',created_at,'updated',updated_at) AS state FROM fleet_workspace_repository_refs ORDER BY workspace_id,ordinal"
    ).fetch_all(scope.executor()).await.map_err(StoreError::backend)?.into_iter()
        .map(|row| row.try_get("state").map_err(StoreError::backend)).collect::<Result<Vec<String>,_>>()?);
        Ok(rows)
    }
}

/// An unused dispatch capability lets stopped-owner readers borrow TursoScope.
pub struct ReadOnlyHost;
impl Clock for ReadOnlyHost {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(0)
    }
}
impl IdSource for ReadOnlyHost {
    fn new_id(&self) -> durable_steps::Uuid {
        durable_steps::Uuid::now_v7()
    }
}
impl WakeScheduler for ReadOnlyHost {
    async fn wake_at(&self, _: SyncTime) -> Result<(), WakeFailure> {
        Ok(())
    }
}
impl Host for ReadOnlyHost {
    async fn dispatch<T: CallTarget>(
        &self,
        _: T,
        _: T::Request,
        _: CallId,
    ) -> Result<T::Answer, CallFailure> {
        Err(CallFailure::NotSent {
            reason: NotSentReason::Overloaded,
        })
    }
}
