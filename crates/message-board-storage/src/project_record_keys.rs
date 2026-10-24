//! Closed record kinds select physical keys from their owning typed tables.
use crate::ProjectStorageError;
use message_board::{RecordId, RecordKind, RecordRef};
use sqlx::Executor;
use sqlx_turso::Turso;

pub(crate) async fn record_keys<TScope>(
    transaction: &mut TScope,
    kind: RecordKind,
    after: &str,
    limit: u64,
) -> Result<Vec<RecordRef>, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: Executor<'scope, Database = Turso>,
{
    let limit = i64::try_from(limit).map_err(|_| ProjectStorageError::InvalidFeed)?;
    let keys = match kind {
        RecordKind::Project => sqlx_turso::query_scalar!(r#"SELECT project_id AS "id!: String" FROM project_state WHERE project_id>? ORDER BY project_id LIMIT ?"#, after, limit).fetch_all(&mut *transaction).await?,
        RecordKind::Workstream => sqlx_turso::query_scalar!(r#"SELECT workstream_id AS "id!: String" FROM workstreams WHERE workstream_id>? ORDER BY workstream_id LIMIT ?"#, after, limit).fetch_all(&mut *transaction).await?,
        RecordKind::Task => sqlx_turso::query_scalar!(r#"SELECT task_id AS "id!: String" FROM tasks WHERE task_id>? ORDER BY task_id LIMIT ?"#, after, limit).fetch_all(&mut *transaction).await?,
        RecordKind::Subtask => sqlx_turso::query_scalar!(r#"SELECT subtask_id AS "id!: String" FROM subtasks WHERE subtask_id>? ORDER BY subtask_id LIMIT ?"#, after, limit).fetch_all(&mut *transaction).await?,
        RecordKind::TaskLinkEvent => sqlx_turso::query_scalar!(r#"SELECT event_id AS "id!: String" FROM task_link_events WHERE event_id>? ORDER BY event_id LIMIT ?"#, after, limit).fetch_all(&mut *transaction).await?,
        RecordKind::Thread => sqlx_turso::query_scalar!(r#"SELECT thread_id AS "id!: String" FROM threads WHERE thread_id>? ORDER BY thread_id LIMIT ?"#, after, limit).fetch_all(&mut *transaction).await?,
        RecordKind::Post => sqlx_turso::query_scalar!(r#"SELECT post_id AS "id!: String" FROM posts WHERE post_id>? ORDER BY post_id LIMIT ?"#, after, limit).fetch_all(&mut *transaction).await?,
        RecordKind::ThreadRefiled => sqlx_turso::query_scalar!(r#"SELECT refiling_id AS "id!: String" FROM thread_refilings WHERE refiling_id>? ORDER BY refiling_id LIMIT ?"#, after, limit).fetch_all(&mut *transaction).await?,
        RecordKind::RoleAssignment => sqlx_turso::query_scalar!(r#"SELECT assignment_id AS "id!: String" FROM role_assignments WHERE assignment_id>? ORDER BY assignment_id LIMIT ?"#, after, limit).fetch_all(&mut *transaction).await?,
        RecordKind::OwnerGrant => sqlx_turso::query_scalar!(r#"SELECT grant_id AS "id!: String" FROM owner_grants WHERE grant_id>? ORDER BY grant_id LIMIT ?"#, after, limit).fetch_all(&mut *transaction).await?,
        RecordKind::StandDownAcknowledgement => sqlx_turso::query_scalar!(r#"SELECT acknowledgement_id AS "id!: String" FROM stand_down_acknowledgements WHERE acknowledgement_id>? ORDER BY acknowledgement_id LIMIT ?"#, after, limit).fetch_all(&mut *transaction).await?,
        RecordKind::Fact => sqlx_turso::query_scalar!(r#"SELECT fact_id AS "id!: String" FROM facts WHERE fact_id>? ORDER BY fact_id LIMIT ?"#, after, limit).fetch_all(&mut *transaction).await?,
        RecordKind::Move => sqlx_turso::query_scalar!(r#"SELECT move_id AS "id!: String" FROM moves WHERE move_id>? ORDER BY move_id LIMIT ?"#, after, limit).fetch_all(&mut *transaction).await?,
        RecordKind::PlanRevision => sqlx_turso::query_scalar!(r#"SELECT revision_id AS "id!: String" FROM plan_revisions WHERE revision_id>? ORDER BY revision_id LIMIT ?"#, after, limit).fetch_all(&mut *transaction).await?,
        RecordKind::WorkTrailRow => sqlx_turso::query_scalar!(r#"SELECT row_id AS "id!: String" FROM work_trail_rows WHERE row_id>? ORDER BY row_id LIMIT ?"#, after, limit).fetch_all(&mut *transaction).await?,
        RecordKind::TrailRedaction => sqlx_turso::query_scalar!(r#"SELECT record_id AS "id!: String" FROM project_feed WHERE record_id>? AND record_kind='trailRedaction' ORDER BY record_id LIMIT ?"#, after, limit).fetch_all(&mut *transaction).await?,
        RecordKind::Request => sqlx_turso::query_scalar!(r#"SELECT request_id AS "id!: String" FROM request_decisions WHERE request_id>? ORDER BY request_id LIMIT ?"#, after, limit).fetch_all(&mut *transaction).await?,
        RecordKind::NotifyIntent => sqlx_turso::query_scalar!(r#"SELECT intent_id AS "id!: String" FROM notify_intents WHERE intent_id>? ORDER BY intent_id LIMIT ?"#, after, limit).fetch_all(&mut *transaction).await?,
        RecordKind::SubscriptionMembership => sqlx_turso::query_scalar!(r#"SELECT membership_id AS "id!: String" FROM subscription_memberships WHERE membership_id>? ORDER BY membership_id LIMIT ?"#, after, limit).fetch_all(&mut *transaction).await?,
        RecordKind::ReadMark => sqlx_turso::query_scalar!(r#"SELECT mark_id AS "id!: String" FROM read_marks WHERE mark_id>? ORDER BY mark_id LIMIT ?"#, after, limit).fetch_all(&mut *transaction).await?,
        RecordKind::Workspace | RecordKind::Board | RecordKind::Adr | RecordKind::ProcessDefinition => return Err(ProjectStorageError::InvalidFeed),
    };
    keys.into_iter()
        .map(|id| {
            Ok(RecordRef {
                kind,
                id: RecordId::try_from(id).map_err(|_| ProjectStorageError::InvalidFeed)?,
            })
        })
        .collect()
}
