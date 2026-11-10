//! Read the current typed Fleet rows through their production decoders.
use durable_steps::{CallFailure, CallId, CallTarget, Host, NotSentReason};
use durable_steps::{storage::TransactionScope, turso_store::TursoScope};
use message_board::{RecordId, RecordKind, RecordRef};
use runtime_primitives::{Clock, IdSource, SyncTime, WakeFailure, WakeScheduler};
use sqlx_turso::sqlx::{ConnectOptions, Connection, Row};
use std::path::Path;

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

// The borrowed read-only connection owns no engine, migrations or dispatch route.
struct ReadbackHost;
impl Clock for ReadbackHost {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(0)
    }
}
impl IdSource for ReadbackHost {
    fn new_id(&self) -> durable_steps::Uuid {
        durable_steps::Uuid::now_v7()
    }
}
impl WakeScheduler for ReadbackHost {
    async fn wake_at(&self, _: SyncTime) -> Result<(), WakeFailure> {
        Ok(())
    }
}
impl Host for ReadbackHost {
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

async fn open(root: &Path) -> TestResult<sqlx_turso::TursoConnection> {
    Ok(sqlx_turso::TursoConnectOptions::new()
        .filename(root.join("fleet.db"))
        .create_if_missing(false)
        .read_only(true)
        .connect()
        .await?)
}

pub async fn summaries(root: &Path) -> TestResult<Vec<String>> {
    let mut connection = open(root).await?;
    let observed = summary_rows(&mut connection).await;
    let closed = connection.close().await;
    let rows = observed?;
    closed?;
    Ok(rows)
}

async fn summary_rows(connection: &mut sqlx_turso::TursoConnection) -> TestResult<Vec<String>> {
    let mut transaction = connection.begin().await?;
    let mut scope = TursoScope::new(&mut transaction, &ReadbackHost);
    let keys = sqlx_turso::sqlx::query::<sqlx_turso::Turso>(
        "SELECT project_id FROM fleet_project_directory WHERE copy_epoch IS NOT NULL ORDER BY project_id",
    )
    .fetch_all(scope.executor())
    .await?;
    let mut result = Vec::new();
    for row in keys {
        let id: String = row.try_get("project_id")?;
        let summary = collaboration_central_core::read_fleet_copy_summary_in_scope(
            &mut scope,
            id.try_into()?,
        )
        .await?
        .ok_or("committed Fleet copy summary missing")?;
        result.push(serde_json::to_string(&summary)?);
    }
    transaction.commit().await?;
    Ok(result)
}

pub async fn application_state(root: &Path) -> TestResult<Vec<String>> {
    let mut connection = open(root).await?;
    let observed = application_rows(&mut connection).await;
    let closed = connection.close().await;
    let rows = observed?;
    closed?;
    Ok(rows)
}

async fn application_rows(connection: &mut sqlx_turso::TursoConnection) -> TestResult<Vec<String>> {
    let mut transaction = connection.begin().await?;
    let mut scope = TursoScope::new(&mut transaction, &ReadbackHost);
    let keys = sqlx_turso::sqlx::query::<sqlx_turso::Turso>(
        "SELECT 'workspace' AS kind,workspace_id AS id,import_digest FROM fleet_workspaces UNION ALL SELECT 'board',board_id,import_digest FROM fleet_boards UNION ALL SELECT 'project',project_id,import_digest FROM fleet_project_directory ORDER BY kind,id",
    )
    .fetch_all(scope.executor())
    .await?;
    let mut result = Vec::new();
    for row in keys {
        let kind: String = row.try_get("kind")?;
        let id: String = row.try_get("id")?;
        let digest: Option<String> = row.try_get("import_digest")?;
        let reference = RecordRef {
            kind: match kind.as_str() {
                "workspace" => RecordKind::Workspace,
                "board" => RecordKind::Board,
                "project" => RecordKind::Project,
                _ => return Err("unexpected Fleet fixture record kind".into()),
            },
            id: RecordId::try_from(id)?,
        };
        let record = collaboration_central_core::read_fleet_record_in_scope(&mut scope, &reference)
            .await?
            .ok_or("canonical Fleet fixture record missing")?;
        result.push(format!(
            "record:{}",
            serde_json::to_string(&(record, digest))?
        ));
    }
    for entry in collaboration_central_core::read_fleet_feed_in_scope(&mut scope).await? {
        result.push(format!("feed:{}", serde_json::to_string(&entry)?));
    }
    // Preserve private feed identities and the exact head/floor as well as the
    // decoded bodies, envelopes, ordered lists and import commitments above.
    for query in [
        "SELECT 'feed-id:' || json_object('id',entry_id,'position',position,'record',record_id) AS value FROM fleet_feed ORDER BY position",
        "SELECT 'head:' || json_object('id',state_id,'name',name,'position',last_position,'floor',retained_through) AS value FROM fleet_feed_state ORDER BY name",
    ] {
        let rows = sqlx_turso::sqlx::query::<sqlx_turso::Turso>(query)
            .fetch_all(scope.executor())
            .await?;
        for row in rows {
            result.push(row.try_get("value")?);
        }
    }
    transaction.commit().await?;
    result.sort();
    Ok(result)
}
