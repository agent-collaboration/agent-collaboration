//! Non-creating inspection of the exact owned interaction schema and migration lineage.
use super::{MIGRATOR, StorageError, read_records, read_revision};
use sqlx_turso::TursoConnection;
use std::collections::BTreeSet;

#[derive(Eq, PartialEq)]
pub(super) enum SchemaAdmission {
    Pristine,
    Owned,
}

struct SchemaObjectRow {
    name: String,
    object_type: String,
    sql: Option<String>,
}

struct MigrationProofRow {
    version: i64,
    success: i64,
    checksum: Vec<u8>,
    version_storage: String,
    success_storage: String,
    checksum_storage: String,
}

#[derive(sqlx::FromRow)]
struct SchemaColumnRow {
    name: String,
    column_type: String,
    required: i64,
    primary_key: i64,
}

fn normalize_schema_sql(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_whitespace() && *character != ';')
        .map(|character| character.to_ascii_lowercase())
        .collect()
}

const MAIL_OUTCOME_OBSERVATIONS_VERSION: i64 = 202610210001;
const HELD_MAIL_READINESS_VERSION: i64 = 202610210002;
const SCHEDULED_RETRY_READINESS_VERSION: i64 = 202610210003;
const MAIL_ROUTE_EVIDENCE_VERSION: i64 = 202610210004;
const SETTLED_ANSWER_RETENTION_VERSION: i64 = 202610310001;
const APPROVAL_ROUTES_VERSION: i64 = 202610310002;

fn expected_table_sql(
    table_name: &str,
    has_mail_observations: bool,
    has_route_evidence: bool,
    has_settled_answer_retention: bool,
) -> Result<String, StorageError> {
    let prefix = format!("CREATE TABLE {table_name} (");
    let statement = MIGRATOR
        .iter()
        .flat_map(|migration| migration.sql.as_str().split(';'))
        .find(|statement| statement.trim_start().starts_with(&prefix))
        .ok_or(StorageError::InvalidSchema)?;
    let mut expected = normalize_schema_sql(statement);
    if has_mail_observations {
        let column = match table_name {
            "router_pushes" => Some("last_outcome_at"),
            "local_delivery_retries" => Some("scheduled_retry_at"),
            _ => None,
        };
        if let Some(column) = column {
            let preceding = expected
                .strip_suffix(")strict")
                .ok_or(StorageError::InvalidSchema)?;
            // Turso reconstructs these two owned tables during ADD COLUMN and
            // writes the primary-key clauses in this order. Admit that exact
            // post-migration DDL only when its ledger version is present.
            let preceding = preceding.replace("textprimarykeynotnull", "textnotnullprimarykey");
            expected = format!("{preceding},{column}text)strict");
        }
    }
    if has_route_evidence && table_name == "router_pushes" {
        let preceding = expected
            .strip_suffix(")strict")
            .ok_or(StorageError::InvalidSchema)?;
        expected = format!("{preceding},last_route_evidence_jsontext)strict");
    }
    if has_settled_answer_retention {
        let column = match table_name {
            "message_send_answers" | "local_cancel_deliveries" => Some("settled_at_ms"),
            _ => None,
        };
        if let Some(column) = column {
            let strict = expected.ends_with(")strict");
            let suffix = if strict { ")strict" } else { ")" };
            let preceding = expected
                .strip_suffix(suffix)
                .ok_or(StorageError::InvalidSchema)?;
            let preceding = preceding.replace("textprimarykeynotnull", "textnotnullprimarykey");
            expected = format!("{preceding},{column}integer{suffix}");
        }
    }
    Ok(expected)
}

pub(super) async fn inspect_schema(
    connection: &mut TursoConnection,
) -> Result<SchemaAdmission, StorageError> {
    let objects = sqlx_turso::query_as!(SchemaObjectRow,
        "SELECT name AS \"name!\", type AS \"object_type!\", sql FROM sqlite_schema WHERE name NOT GLOB 'sqlite_*' ORDER BY name")
        .fetch_all(&mut *connection).await.map_err(|_| StorageError::InvalidSchema)?;
    let user_version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&mut *connection)
        .await
        .map_err(|_| StorageError::InvalidSchema)?;
    let application_id: i64 = sqlx::query_scalar("PRAGMA application_id")
        .fetch_one(&mut *connection)
        .await
        .map_err(|_| StorageError::InvalidSchema)?;
    if user_version != 0 || application_id != 0 {
        return Err(StorageError::InvalidSchema);
    }
    if objects.is_empty() {
        return Ok(SchemaAdmission::Pristine);
    }
    let migrations = sqlx_turso::query_as!(MigrationProofRow,
        "SELECT version AS \"version!\", success AS \"success!: i64\", checksum AS \"checksum!: Vec<u8>\", typeof(version) AS \"version_storage!: String\", typeof(success) AS \"success_storage!: String\", typeof(checksum) AS \"checksum_storage!: String\" FROM _sqlx_migrations ORDER BY version",
    ).fetch_all(&mut *connection).await.map_err(|_| StorageError::InvalidSchema)?;
    if migrations.is_empty()
        || migrations.len() > MIGRATOR.iter().count()
        || migrations
            .iter()
            .zip(MIGRATOR.iter())
            .any(|(actual, expected)| {
                actual.version != expected.version
                    || actual.success != 1
                    || actual.version_storage != "integer"
                    || actual.success_storage != "integer"
                    || actual.checksum_storage != "blob"
                    || actual.checksum.as_slice() != expected.checksum.as_ref()
            })
    {
        return Err(StorageError::InvalidSchema);
    }
    let has_mail_observations = migrations
        .iter()
        .any(|row| row.version == MAIL_OUTCOME_OBSERVATIONS_VERSION);
    let has_held_readiness = migrations
        .iter()
        .any(|row| row.version == HELD_MAIL_READINESS_VERSION);
    let has_scheduled_readiness = migrations
        .iter()
        .any(|row| row.version == SCHEDULED_RETRY_READINESS_VERSION);
    let has_route_evidence = migrations
        .iter()
        .any(|row| row.version == MAIL_ROUTE_EVIDENCE_VERSION);
    let has_settled_answer_retention = migrations
        .iter()
        .any(|row| row.version == SETTLED_ANSWER_RETENTION_VERSION);
    let has_approval_routes = migrations
        .iter()
        .any(|row| row.version == APPROVAL_ROUTES_VERSION);
    let history_tables = BTreeSet::from([
        "_sqlx_migrations",
        "interaction_history_revision",
        "typed_interaction_history",
    ]);
    let message_tables = [
        "router_pushes",
        "message_send_answers",
        "subscription_delivery_progress",
        "board_intent_progress",
        "board_intent_scan_cursors",
        "local_cancel_deliveries",
    ];
    // Reserved names at the pinned library revision. The library's public
    // migration owners validate their lineages in this file's transaction.
    let library_tables = [
        "_fleet_sync_migrations",
        "_fleet_sync_operations",
        "_fleet_sync_steps",
        "_fleet_sync_schema",
        "_fleet_sync_delivery_migrations",
        "_fleet_sync_mail_items",
        "_fleet_sync_mail_controls",
        "_fleet_sync_mail_state",
        "_fleet_sync_outbox_items",
        "_fleet_sync_inbox_items",
        "_fleet_sync_inbox_state",
        "_fleet_sync_reports",
        "_fleet_sync_status_owed",
        "_fleet_sync_status_events",
        "_fleet_sync_status_state",
        "_fleet_sync_delivery_receipts",
        "_fleet_sync_mail_cancel_links",
        "_fleet_sync_outbox_cancel_links",
        "_fleet_sync_outbox_status",
        "_fleet_sync_inbox_cancel_links",
    ];
    let tables: BTreeSet<_> = objects
        .iter()
        .filter(|object| object.object_type == "table")
        .map(|object| object.name.as_str())
        .collect();
    let has_delivery_retries = tables.contains("local_delivery_retries");
    let has_installation_directory = tables.contains("installation_peer_directory");
    let has_submission_epochs = tables.contains("mail_submission_epochs");
    let has_retirement_progress = tables.contains("mail_retirement_progress");
    let has_pending_poll = tables.contains("pending_poll_mail");
    let has_read_contexts = migrations.iter().any(|row| row.version == 202610220001);
    if has_installation_directory && !has_delivery_retries {
        return Err(StorageError::InvalidSchema);
    }
    if has_submission_epochs && !has_installation_directory {
        return Err(StorageError::InvalidSchema);
    }
    if has_retirement_progress && !has_submission_epochs {
        return Err(StorageError::InvalidSchema);
    }
    if has_pending_poll && !has_retirement_progress {
        return Err(StorageError::InvalidSchema);
    }
    let mut composed_tables = history_tables.clone();
    composed_tables.extend(message_tables);
    composed_tables.extend(library_tables);
    if has_delivery_retries {
        composed_tables.insert("local_delivery_retries");
    }
    if has_installation_directory {
        composed_tables.insert("installation_peer_directory");
    }
    if has_submission_epochs {
        composed_tables.insert("mail_submission_epochs");
    }
    if has_retirement_progress {
        composed_tables.insert("mail_retirement_progress");
    }
    if has_pending_poll {
        composed_tables.insert("pending_poll_mail");
    }
    if has_read_contexts {
        composed_tables.insert("mail_read_contexts");
    }
    if has_approval_routes {
        composed_tables.insert("approval_routes");
    }
    let composed = tables == composed_tables;
    if tables != history_tables && !composed {
        return Err(StorageError::InvalidSchema);
    }
    let mut expected_indexes = BTreeSet::from([
        "router_pushes_target_state",
        "router_pushes_created",
        "router_pushes_origin_ref",
        "router_pushes_mail_retention",
        "subscription_delivery_progress_identity",
        "board_intent_progress_identity",
        "_fleet_sync_operations_due",
        "_fleet_sync_mail_deadlines",
    ]);
    if has_settled_answer_retention {
        for index_name in [
            "message_send_answers_settled",
            "local_cancel_deliveries_settled",
        ] {
            expected_indexes.insert(index_name);
            let actual = objects
                .iter()
                .find(|object| object.object_type == "index" && object.name == index_name)
                .and_then(|object| object.sql.as_deref())
                .ok_or(StorageError::InvalidSchema)?;
            let expected = MIGRATOR
                .iter()
                .flat_map(|migration| migration.sql.as_str().split(';'))
                .find(|sql| {
                    sql.trim_start()
                        .starts_with(&format!("CREATE INDEX {index_name} "))
                })
                .ok_or(StorageError::InvalidSchema)?;
            if normalize_schema_sql(actual) != normalize_schema_sql(expected) {
                return Err(StorageError::InvalidSchema);
            }
        }
    }
    if has_approval_routes {
        expected_indexes.insert("approval_routes_last_used");
        let actual = objects
            .iter()
            .find(|object| {
                object.object_type == "index" && object.name == "approval_routes_last_used"
            })
            .and_then(|object| object.sql.as_deref())
            .ok_or(StorageError::InvalidSchema)?;
        let expected = MIGRATOR
            .iter()
            .flat_map(|migration| migration.sql.as_str().split(';'))
            .find(|sql| {
                sql.trim_start()
                    .starts_with("CREATE INDEX approval_routes_last_used ")
            })
            .ok_or(StorageError::InvalidSchema)?;
        if normalize_schema_sql(actual) != normalize_schema_sql(expected) {
            return Err(StorageError::InvalidSchema);
        }
    }
    if has_read_contexts {
        expected_indexes.insert("mail_read_context_conversation");
        let actual = objects
            .iter()
            .find(|object| {
                object.object_type == "index" && object.name == "mail_read_context_conversation"
            })
            .and_then(|object| object.sql.as_deref())
            .ok_or(StorageError::InvalidSchema)?;
        let expected = MIGRATOR
            .iter()
            .flat_map(|migration| migration.sql.as_str().split(';'))
            .find(|sql| {
                sql.trim_start()
                    .starts_with("CREATE INDEX mail_read_context_conversation ")
            })
            .ok_or(StorageError::InvalidSchema)?;
        if normalize_schema_sql(actual) != normalize_schema_sql(expected) {
            return Err(StorageError::InvalidSchema);
        }
    }
    // The public SDK migrator upgrades its genuine preceding prefixes. Accept
    // its added index only with the public migration's exact DDL; inspecting an
    // applied-but-missing SDK object belongs to that migration owner (963).
    if let Some(index) = objects.iter().find(|object| {
        object.object_type == "index" && object.name == "_fleet_sync_inbox_accounting"
    }) {
        let expected = fleet_sync_delivery::DELIVERY_MIGRATOR
            .iter()
            .flat_map(|migration| migration.sql.as_str().split(';'))
            .find(|statement| {
                statement
                    .trim_start()
                    .starts_with("CREATE INDEX _fleet_sync_inbox_accounting")
            })
            .ok_or(StorageError::InvalidSchema)?;
        let actual = index.sql.as_deref().ok_or(StorageError::InvalidSchema)?;
        if normalize_schema_sql(actual) != normalize_schema_sql(expected) {
            return Err(StorageError::InvalidSchema);
        }
        expected_indexes.insert("_fleet_sync_inbox_accounting");
    }
    if has_held_readiness {
        expected_indexes.insert("router_pushes_held_mail_readiness");
        let index = objects
            .iter()
            .find(|object| {
                object.object_type == "index" && object.name == "router_pushes_held_mail_readiness"
            })
            .and_then(|object| object.sql.as_deref())
            .ok_or(StorageError::InvalidSchema)?;
        let expected = MIGRATOR
            .iter()
            .find(|migration| migration.version == HELD_MAIL_READINESS_VERSION)
            .ok_or(StorageError::InvalidSchema)?;
        if normalize_schema_sql(index) != normalize_schema_sql(expected.sql.as_str()) {
            return Err(StorageError::InvalidSchema);
        }
    }
    if has_scheduled_readiness {
        expected_indexes.insert("local_delivery_retries_readiness");
        let index = objects
            .iter()
            .find(|object| {
                object.object_type == "index" && object.name == "local_delivery_retries_readiness"
            })
            .and_then(|object| object.sql.as_deref())
            .ok_or(StorageError::InvalidSchema)?;
        let expected = MIGRATOR
            .iter()
            .find(|migration| migration.version == SCHEDULED_RETRY_READINESS_VERSION)
            .ok_or(StorageError::InvalidSchema)?;
        if normalize_schema_sql(index) != normalize_schema_sql(expected.sql.as_str()) {
            return Err(StorageError::InvalidSchema);
        }
    }
    if has_pending_poll {
        expected_indexes.insert("pending_poll_mail_recipient");
        let actual = objects
            .iter()
            .find(|object| {
                object.object_type == "index" && object.name == "pending_poll_mail_recipient"
            })
            .and_then(|object| object.sql.as_deref())
            .ok_or(StorageError::InvalidSchema)?;
        let expected = MIGRATOR
            .iter()
            .flat_map(|migration| migration.sql.as_str().split(';'))
            .find(|sql| {
                sql.trim_start()
                    .starts_with("CREATE INDEX pending_poll_mail_recipient ")
            })
            .ok_or(StorageError::InvalidSchema)?;
        if normalize_schema_sql(actual) != normalize_schema_sql(expected) {
            return Err(StorageError::InvalidSchema);
        }
    }
    let indexes: BTreeSet<_> = objects
        .iter()
        .filter(|object| object.object_type == "index")
        .map(|object| object.name.as_str())
        .collect();
    if objects
        .iter()
        .any(|object| !matches!(object.object_type.as_str(), "table" | "index"))
        || (composed && indexes != expected_indexes)
        || (!composed && !indexes.is_empty())
    {
        return Err(StorageError::InvalidSchema);
    }
    for object in objects.iter().filter(|object| {
        object.object_type == "table"
            && (matches!(
                object.name.as_str(),
                "interaction_history_revision" | "typed_interaction_history"
            ) || message_tables.contains(&object.name.as_str())
                || matches!(
                    object.name.as_str(),
                    "local_delivery_retries"
                        | "installation_peer_directory"
                        | "mail_submission_epochs"
                        | "mail_retirement_progress"
                        | "pending_poll_mail"
                        | "mail_read_contexts"
                        | "approval_routes"
                ))
    }) {
        let actual = object.sql.as_deref().ok_or(StorageError::InvalidSchema)?;
        let expected = expected_table_sql(
            &object.name,
            has_mail_observations,
            has_route_evidence,
            has_settled_answer_retention,
        )?;
        if normalize_schema_sql(actual) != expected {
            return Err(StorageError::InvalidSchema);
        }
    }
    // PRAGMA shape inspection is dynamic; owned history rows use checked queries.
    let columns = sqlx::query_as::<_, SchemaColumnRow>(
        "SELECT name, type AS column_type, \"notnull\" AS required, pk AS primary_key FROM pragma_table_info('_sqlx_migrations') ORDER BY cid")
        .fetch_all(&mut *connection).await.map_err(|_| StorageError::InvalidSchema)?;
    let expected_columns = [
        ("version", "BIGINT", 0, 1),
        ("description", "TEXT", 1, 0),
        ("installed_on", "TIMESTAMP", 1, 0),
        ("success", "BOOLEAN", 1, 0),
        ("checksum", "BLOB", 1, 0),
        ("execution_time", "BIGINT", 1, 0),
    ];
    if columns.len() != expected_columns.len()
        || columns.iter().zip(expected_columns).any(
            |(actual, (name, column_type, required, primary_key))| {
                actual.name != name
                    || actual.column_type != column_type
                    || actual.required != required
                    || actual.primary_key != primary_key
            },
        )
    {
        return Err(StorageError::InvalidSchema);
    }
    // Admit exact preceding migration prefixes. Each optional final table must
    // agree with its ledger row; mixed or partially applied schemas still refuse.
    let expected_migrations = if composed {
        MIGRATOR.iter().count()
            - usize::from(!has_delivery_retries)
            - usize::from(!has_installation_directory)
            - usize::from(!has_submission_epochs)
            - usize::from(!has_mail_observations)
            - usize::from(!has_held_readiness)
            - usize::from(!has_scheduled_readiness)
            - usize::from(!has_route_evidence)
            - usize::from(!has_retirement_progress)
            - usize::from(!has_pending_poll)
            - usize::from(!has_read_contexts)
            - usize::from(!has_settled_answer_retention)
            - usize::from(!has_approval_routes)
    } else {
        1
    };
    if migrations.len() != expected_migrations {
        return Err(StorageError::InvalidSchema);
    }
    read_revision(connection).await?;
    read_records(connection).await?;
    Ok(SchemaAdmission::Owned)
}
