//! Admit only exact known SQLx prefixes, then read the retired file without mutation.
use crate::{
    MessageActivityMap,
    message_copy::MessageUpgradeError,
    message_rows::{MessageRows, invalid},
};
use rusqlite::{Connection, OpenFlags};
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha384};
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
    let path = path.to_owned();
    let mapping = mapping.clone();
    tokio::task::spawn_blocking(move || read_closed(&path, &mapping)).await?
}

fn read_closed(
    path: &Path,
    mapping: &MessageActivityMap,
) -> Result<MessageSource, MessageUpgradeError> {
    if !path.try_exists()? {
        return Ok(MessageSource::Absent);
    }
    crate::legacy_reader::refuse_open_file(path).map_err(|_| MessageUpgradeError::OpenOwner)?;
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.execute_batch("PRAGMA query_only=ON; BEGIN;")?;
    let version = admit(&connection)?;
    if version >= DROP_VERSION {
        return Ok(MessageSource::Moved);
    }
    let has = |name: &str| {
        connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name=?)",
            [name],
            |row| row.get::<_, bool>(0),
        )
    };
    if has("mail_items")? {
        let count: i64 =
            connection.query_row("SELECT count(*) FROM mail_items", [], |row| row.get(0))?;
        if count != 0 {
            return Err(invalid("nonempty pre-release mail_items"));
        }
    }
    let mut result = MessageRows::default();
    if has("router_pushes")? {
        if version >= 20_261_016_000_200 {
            let invalid_retention: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM router_pushes WHERE retention NOT IN ('creationAge','mailTerminalAge'))", [], |row| row.get(0))?;
            if invalid_retention {
                return Err(invalid("push.retention"));
            }
        }
        result.pushes = rows(&connection, PUSH_READ)?;
        crate::message_origin_map::translate(&mut result, mapping)?;
        mapping.translate(&mut result)?;
    }
    if has("subscription_delivery_progress")? {
        result.subscriptions = rows(&connection, SUBSCRIPTION_READ)?;
        for progress in &mut result.subscriptions {
            progress.project_payload()?;
        }
    }
    if has("board_intent_progress")? {
        if version < VERSIONED_INTENTS {
            let count: i64 =
                connection.query_row("SELECT count(*) FROM board_intent_progress", [], |row| {
                    row.get(0)
                })?;
            if count != 0 {
                return Err(invalid("unversioned intent progress"));
            }
        } else {
            result.intents = rows(&connection, INTENT_READ)?;
            for intent in &result.intents {
                intent.validate()?;
            }
        }
    }
    if has("board_intent_scan_cursors")? {
        result.cursors = rows(&connection, CURSOR_READ)?;
        for cursor in &result.cursors {
            cursor.validate()?;
        }
    }
    Ok(MessageSource::Legacy(result))
}

fn admit(source: &Connection) -> Result<i64, MessageUpgradeError> {
    let mut statement = source.prepare(
        "SELECT version, description, success, checksum FROM _sqlx_migrations ORDER BY version",
    )?;
    let applied = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>("version")?,
                row.get::<_, String>("description")?,
                row.get::<_, i64>("success")?,
                row.get::<_, Vec<u8>>("checksum")?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if applied.is_empty() || applied.len() > MIGRATIONS.len() {
        return Err(invalid("automation migration history"));
    }
    let expected = Connection::open_in_memory()?;
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
        expected.execute_batch(sql)?;
    }
    if objects(source)? != objects(&expected)? {
        return Err(invalid("automation schema for migration prefix"));
    }
    applied
        .last()
        .map(|record| record.0)
        .ok_or_else(|| invalid("automation migration history"))
}
fn objects(connection: &Connection) -> Result<Vec<(String, String)>, MessageUpgradeError> {
    let mut statement = connection.prepare("SELECT name,sql FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%' AND name != '_sqlx_migrations' ORDER BY name")?;
    Ok(statement
        .query_map([], |row| {
            let sql: String = row.get("sql")?;
            Ok((
                row.get("name")?,
                sql.split_whitespace().collect::<Vec<_>>().join(" "),
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?)
}
fn rows<T: DeserializeOwned>(
    connection: &Connection,
    sql: &str,
) -> Result<Vec<T>, MessageUpgradeError> {
    let mut statement = connection.prepare(sql)?;
    let encoded = statement
        .query_map([], |row| row.get::<_, String>("payload"))?
        .collect::<Result<Vec<_>, _>>()?;
    encoded
        .into_iter()
        .map(|value| serde_json::from_str(&value).map_err(|_| invalid("message columns")))
        .collect()
}

const PUSH_READ: &str = "SELECT json_object('push_id',push_id,'kind',kind,'origin_kind',origin_kind,'origin_service_id',origin_service_id,'origin_endpoint_id',origin_endpoint_id,'origin_session_id',origin_session_id,'origin_router_ref',origin_router_ref,'target_service_id',target_service_id,'target_endpoint_id',target_endpoint_id,'target_session_id',target_session_id,'dm_delivery_mode',dm_delivery_mode,'dm_generation_guard_json',dm_generation_guard_json,'reply_to_push_id',reply_to_push_id,'header_facts_json',header_facts_json,'body',body,'ranges_json',ranges_json,'delivery_state',delivery_state,'last_outcome_json',last_outcome_json,'created_at',created_at,'settled_at',settled_at,'read_at',read_at) AS payload FROM router_pushes ORDER BY push_id";

const SUBSCRIPTION_READ: &str = "SELECT json_object('project_id',project_id,'membership_id',membership_id,'activated_epoch',activated_epoch,'activated_position',activated_position,'payload',payload) AS payload FROM subscription_delivery_progress ORDER BY project_id,membership_id,activated_epoch,activated_position";

const INTENT_READ: &str = "SELECT json_object('project_id',project_id,'intent_id',intent_id,'state_epoch',state_epoch,'state_position',state_position) AS payload FROM board_intent_progress ORDER BY project_id,intent_id,state_epoch,state_position";

const CURSOR_READ: &str = "SELECT json_object('project_id',project_id,'epoch',epoch,'position',position) AS payload FROM board_intent_scan_cursors ORDER BY project_id";
