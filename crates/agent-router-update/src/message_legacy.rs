//! Admit only exact known SQLx prefixes, then read the retired file without mutation.
use crate::{
    MessageActivityMap,
    message_copy::MessageUpgradeError,
    message_rows::{MessageRows, invalid},
};
use futures_util::TryStreamExt;
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha384};
use sqlx::{Connection, Row, SqliteConnection, sqlite::SqliteConnectOptions};
use std::path::Path;

pub(crate) enum MessageSource {
    Absent,
    Moved,
    Legacy(MessageRows),
}
const DROP_VERSION: i64 = 20_261_021_000_100;
const VERSIONED_INTENTS: i64 = 20_261_015_000_200;
const MIGRATIONS: &[(i64, &str, &str)] = &[
    (
        20260910000000,
        "automation v1",
        include_str!("../../automation-storage/migrations/20260910000000_automation_v1.sql"),
    ),
    (
        20260924000000,
        "delivery outcome receipt",
        include_str!(
            "../../automation-storage/migrations/20260924000000_delivery_outcome_receipt.sql"
        ),
    ),
    (
        20260928000000,
        "latest agent sender",
        include_str!("../../automation-storage/migrations/20260928000000_latest_agent_sender.sql"),
    ),
    (
        20260930000000,
        "router pushes",
        include_str!("../../automation-storage/migrations/20260930000000_router_pushes.sql"),
    ),
    (
        20261012000100,
        "subscription delivery progress",
        include_str!(
            "../../automation-storage/migrations/20261012000100_subscription_delivery_progress.sql"
        ),
    ),
    (
        20261013000100,
        "board intent progress",
        include_str!(
            "../../automation-storage/migrations/20261013000100_board_intent_progress.sql"
        ),
    ),
    (
        20261015000100,
        "board intent scan cursors",
        include_str!(
            "../../automation-storage/migrations/20261015000100_board_intent_scan_cursors.sql"
        ),
    ),
    (
        20261015000200,
        "intent state consumption",
        include_str!(
            "../../automation-storage/migrations/20261015000200_intent_state_consumption.sql"
        ),
    ),
    (
        20261016000100,
        "wake target slot",
        include_str!("../../automation-storage/migrations/20261016000100_wake_target_slot.sql"),
    ),
    (
        20261016000200,
        "local mail custody",
        include_str!("../../automation-storage/migrations/20261016000200_local_mail_custody.sql"),
    ),
    (
        20261019000200,
        "owed push operations",
        include_str!("../../automation-storage/migrations/20261019000200_owed_push_operations.sql"),
    ),
    (
        20261021000100,
        "drop message tables",
        include_str!("../../automation-storage/migrations/20261021000100_drop_message_tables.sql"),
    ),
    (
        20261023000100,
        "remote wake requests",
        include_str!("../../automation-storage/migrations/20261023000100_remote_wake_requests.sql"),
    ),
];

pub(crate) async fn read(
    path: &Path,
    mapping: &MessageActivityMap,
) -> Result<MessageSource, MessageUpgradeError> {
    let inspected_path = path.to_owned();
    if !tokio::task::spawn_blocking(move || inspected_path.try_exists()).await?? {
        return Ok(MessageSource::Absent);
    }
    crate::legacy_reader::refuse_open_file(path)
        .await
        .map_err(|_| MessageUpgradeError::OpenOwner)?;
    let mut connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(path)
            .read_only(true)
            .create_if_missing(false)
            .foreign_keys(false)
            .pragma("query_only", "ON"),
    )
    .await?;
    let source_result = read_snapshot(&mut connection, mapping).await;
    let rollback_result = sqlx::raw_sql("ROLLBACK;").execute(&mut connection).await;
    let close_result = connection.close().await;
    let source = source_result?;
    rollback_result?;
    close_result?;
    Ok(source)
}

async fn read_snapshot(
    connection: &mut SqliteConnection,
    mapping: &MessageActivityMap,
) -> Result<MessageSource, MessageUpgradeError> {
    sqlx::raw_sql("BEGIN;").execute(&mut *connection).await?;
    let version = admit(connection).await?;
    if version >= DROP_VERSION {
        return Ok(MessageSource::Moved);
    }
    if has_table(connection, "mail_items").await? {
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM mail_items")
            .fetch_one(&mut *connection)
            .await?;
        if count != 0 {
            return Err(invalid("nonempty pre-release mail_items"));
        }
    }
    let mut result = MessageRows::default();
    if has_table(connection, "router_pushes").await? {
        if version >= 20_261_016_000_200 {
            let invalid_retention: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM router_pushes WHERE retention NOT IN ('creationAge','mailTerminalAge'))").fetch_one(&mut *connection).await?;
            if invalid_retention {
                return Err(invalid("push.retention"));
            }
        }
        result.pushes = rows(connection, PUSH_READ).await?;
        crate::message_origin_map::translate(&mut result, mapping)?;
        mapping.translate(&mut result)?;
    }
    if has_table(connection, "subscription_delivery_progress").await? {
        result.subscriptions = rows(connection, SUBSCRIPTION_READ).await?;
        for progress in &mut result.subscriptions {
            progress.project_payload()?;
        }
    }
    if has_table(connection, "board_intent_progress").await? {
        if version < VERSIONED_INTENTS {
            let count: i64 = sqlx::query_scalar("SELECT count(*) FROM board_intent_progress")
                .fetch_one(&mut *connection)
                .await?;
            if count != 0 {
                return Err(invalid("unversioned intent progress"));
            }
        } else {
            result.intents = rows(connection, INTENT_READ).await?;
            for intent in &result.intents {
                intent.validate()?;
            }
        }
    }
    if has_table(connection, "board_intent_scan_cursors").await? {
        result.cursors = rows(connection, CURSOR_READ).await?;
        for cursor in &result.cursors {
            cursor.validate()?;
        }
    }
    Ok(MessageSource::Legacy(result))
}

async fn has_table(connection: &mut SqliteConnection, name: &str) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name=?)")
        .bind(name)
        .fetch_one(connection)
        .await
}

async fn admit(source: &mut SqliteConnection) -> Result<i64, MessageUpgradeError> {
    let records = sqlx::query(
        "SELECT version, description, success, checksum FROM _sqlx_migrations ORDER BY version",
    )
    .fetch_all(&mut *source)
    .await?;
    let applied = records
        .into_iter()
        .map(|row| {
            Ok((
                row.try_get::<i64, _>("version")?,
                row.try_get::<String, _>("description")?,
                row.try_get::<i64, _>("success")?,
                row.try_get::<Vec<u8>, _>("checksum")?,
            ))
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()?;
    if applied.is_empty() || applied.len() > MIGRATIONS.len() {
        return Err(invalid("automation migration history"));
    }
    let mut expected = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .in_memory(true)
            .create_if_missing(true)
            .foreign_keys(false),
    )
    .await?;
    let admission = async {
        for (index, (version, description, success, checksum)) in applied.iter().enumerate() {
            let (known_version, known_description, sql) = MIGRATIONS
                .get(index)
                .ok_or_else(|| invalid("automation migration history"))?;
            let expected_checksum = Sha384::digest(sql.as_bytes());
            let expected_bytes: &[u8] = expected_checksum.as_ref();
            if version != known_version
                || description != known_description
                || *success != 1
                || checksum.as_slice() != expected_bytes
            {
                return Err(invalid("automation migration identity"));
            }
            sqlx::raw_sql(sqlx::AssertSqlSafe(*sql))
                .execute(&mut expected)
                .await?;
        }
        if objects(source).await? != objects(&mut expected).await? {
            return Err(invalid("automation schema for migration prefix"));
        }
        applied
            .last()
            .map(|record| record.0)
            .ok_or_else(|| invalid("automation migration history"))
    }
    .await;
    let close_result = expected.close().await;
    let version = admission?;
    close_result?;
    Ok(version)
}

async fn objects(
    connection: &mut SqliteConnection,
) -> Result<Vec<(String, String)>, MessageUpgradeError> {
    let records = sqlx::query("SELECT name,sql FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%' AND name != '_sqlx_migrations' ORDER BY name").fetch_all(connection).await?;
    records
        .into_iter()
        .map(|row| {
            let sql: String = row.try_get("sql")?;
            Ok((
                row.try_get("name")?,
                sql.split_whitespace().collect::<Vec<_>>().join(" "),
            ))
        })
        .collect()
}

async fn rows<T: DeserializeOwned>(
    connection: &mut SqliteConnection,
    sql: &str,
) -> Result<Vec<T>, MessageUpgradeError> {
    let mut cursor = sqlx::query(sqlx::AssertSqlSafe(sql)).fetch(connection);
    let mut result = Vec::new();
    while let Some(row) = cursor.try_next().await? {
        let value: String = row.try_get("payload")?;
        result.push(serde_json::from_str(&value).map_err(|_| invalid("message columns"))?);
    }
    Ok(result)
}

const PUSH_READ: &str = "SELECT json_object('push_id',push_id,'kind',kind,'origin_kind',origin_kind,'origin_service_id',origin_service_id,'origin_endpoint_id',origin_endpoint_id,'origin_session_id',origin_session_id,'origin_router_ref',origin_router_ref,'target_service_id',target_service_id,'target_endpoint_id',target_endpoint_id,'target_session_id',target_session_id,'dm_delivery_mode',dm_delivery_mode,'dm_generation_guard_json',dm_generation_guard_json,'reply_to_push_id',reply_to_push_id,'header_facts_json',header_facts_json,'body',body,'ranges_json',ranges_json,'delivery_state',delivery_state,'last_outcome_json',last_outcome_json,'created_at',created_at,'settled_at',settled_at,'read_at',read_at) AS payload FROM router_pushes ORDER BY push_id";

const SUBSCRIPTION_READ: &str = "SELECT json_object('project_id',project_id,'membership_id',membership_id,'activated_epoch',activated_epoch,'activated_position',activated_position,'payload',payload) AS payload FROM subscription_delivery_progress ORDER BY project_id,membership_id,activated_epoch,activated_position";

const INTENT_READ: &str = "SELECT json_object('project_id',project_id,'intent_id',intent_id,'state_epoch',state_epoch,'state_position',state_position) AS payload FROM board_intent_progress ORDER BY project_id,intent_id,state_epoch,state_position";

const CURSOR_READ: &str = "SELECT json_object('project_id',project_id,'epoch',epoch,'position',position) AS payload FROM board_intent_scan_cursors ORDER BY project_id";
