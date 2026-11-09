//! Main's own decoders over one rebuilt object, each in that object's
//! transaction, the way main's central reads them after PD7 step 5:
//!
//! - Fleet: fleet-sync-lease's legacy-row validation over every carried lease
//!   table; `StoredMachine::decode` for each enrolled machine; the canonical
//!   record decoder for every workspace, board, ADR and directory entry; and,
//!   per directory entry, the Copy summary and lease projection decoders.
//! - Copy: fleet-sync-receiver's `copy_status`, which validates the Copy record,
//!   its Copy inspection, and its retained-record inspection (every receiver
//!   record family through the receiver's own readers).
//!
//! The Fleet also gets: repository attachments per workspace (orphans
//! included), the lease incarnation, the feed state and retained range, and
//! each request ledger record. table_coverage.rs lists what covers every table.

use crate::candidate_image::{ObjectImage, TypedValue};
use board_sync_protocol::BoardAnnotation;
use durable_steps::storage::TransactionScope;
use fleet_sync_protocol::{CopyStatusAnswer, DatabaseId};
use fleet_sync_receiver::RetainedFamily;
use message_board::{RecordId, RecordKind, RecordRef};
use serde::Serialize;
use sqlx_core::connection::Connection;
use sqlx_turso::{Turso, TursoConnection};

struct TursoScope<'connection>(&'connection mut TursoConnection);
impl TransactionScope for TursoScope<'_> {
    type Database = Turso;
    type Executor<'query>
        = &'query mut TursoConnection
    where
        Self: 'query;
    fn executor(&mut self) -> Self::Executor<'_> {
        self.0
    }
}

/// One value main refuses: which object, which row or decoder, and main's reason.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Refusal {
    pub object: String,
    pub row: String,
    pub reason: String,
}

/// Graph tables and the record kind main reads each row as, by its id column.
const RECORD_TABLES: [(&str, &str, RecordKind); 4] = [
    ("fleet_workspaces", "workspace_id", RecordKind::Workspace),
    ("fleet_boards", "board_id", RecordKind::Board),
    ("fleet_workspace_adrs", "adr_id", RecordKind::Adr),
    ("fleet_project_directory", "project_id", RecordKind::Project),
];

pub async fn decode_fleet(
    image: &ObjectImage,
    connection: &mut TursoConnection,
) -> Result<Vec<Refusal>, sqlx_core::Error> {
    let object = "fleet".to_owned();
    let mut refusals = Vec::new();
    let mut refuse = |row: String, reason: String| {
        refusals.push(Refusal {
            object: object.clone(),
            row,
            reason,
        });
    };
    let mut transaction = connection.begin().await?;
    let mut scope = TursoScope(&mut transaction);
    if let Err(error) = fleet_sync_lease::validate_legacy_rows(&mut scope).await {
        refuse("lease tables".into(), error.to_string());
    }
    for machine in image.text_column("machine_enrollments", "machine_id") {
        match collaboration_central_core::decode_machine_enrollments_in_scope(&mut scope, &machine)
            .await
        {
            Ok(0) => refuse(
                format!("machine_enrollments {machine}"),
                "main's read finds no enrollment".into(),
            ),
            Ok(_) => {}
            Err(error) => refuse(format!("machine_enrollments {machine}"), error.to_string()),
        }
    }
    for (table, column, kind) in RECORD_TABLES {
        for id in image.text_column(table, column) {
            let row = format!("{table} {id}");
            let reference = match RecordId::try_from(id.clone()) {
                Ok(id) => RecordRef { kind, id },
                Err(error) => {
                    refuse(row, error.to_string());
                    continue;
                }
            };
            match collaboration_central_core::read_fleet_record_in_scope(&mut scope, &reference)
                .await
            {
                Ok(Some(_)) => {}
                Ok(None) => refuse(row, "main's read finds no record".into()),
                Err(error) => refuse(row, error.to_string()),
            }
        }
    }
    // Repository attachments: every workspace that has a record or an attachment.
    let mut workspaces = image.text_column("fleet_workspaces", "workspace_id");
    workspaces.extend(image.text_column("fleet_repository_attachments", "workspace_id"));
    workspaces.sort();
    workspaces.dedup();
    let mut attachments = 0;
    let mut attachment_refused = false;
    for workspace in &workspaces {
        match collaboration_central_core::read_fleet_repository_attachments_in_scope(
            &mut scope, workspace,
        )
        .await
        {
            Ok(decoded) => attachments += decoded,
            Err(error) => {
                attachment_refused = true;
                refuse(
                    format!("fleet_repository_attachments {workspace}"),
                    error.to_string(),
                );
            }
        }
    }
    let stored = image.row_count("fleet_repository_attachments");
    if attachments != stored && !attachment_refused {
        refuse(
            "fleet_repository_attachments".into(),
            format!("main's reads decode {attachments} of {stored} attachments"),
        );
    }
    // The lease incarnation, when the Fleet holds one.
    match collaboration_central_core::read_fleet_lease_incarnation_in_scope(&mut scope).await {
        Ok(None) if image.row_count("lease_incarnation") > 0 => refuse(
            "lease_incarnation".into(),
            "main's read finds no Fleet incarnation".into(),
        ),
        Ok(_) => {}
        Err(error) => refuse("lease_incarnation".into(), error.to_string()),
    }
    // The feed: its state and every retained entry.
    if let Err(error) = collaboration_central_core::read_fleet_feed_in_scope(&mut scope).await {
        refuse("fleet_feed_state".into(), error.to_string());
    }
    // Each request ledger record, with its reasons, evidence and receipts.
    for request in image.text_column("fleet_request_decisions", "request_id") {
        let row = format!("fleet_request_decisions {request}");
        let reference = match RecordId::try_from(request.clone()) {
            Ok(id) => RecordRef {
                kind: RecordKind::Request,
                id,
            },
            Err(error) => {
                refuse(row, error.to_string());
                continue;
            }
        };
        match collaboration_central_core::read_fleet_record_in_scope(&mut scope, &reference).await {
            Ok(Some(_)) => {}
            Ok(None) => refuse(row, "main's read finds no request".into()),
            Err(error) => refuse(row, error.to_string()),
        }
    }
    for project in image.text_column("fleet_project_directory", "project_id") {
        let row = format!("fleet_project_directory {project}");
        let database = match project
            .parse::<uuid::Uuid>()
            .map_err(|error| error.to_string())
            .and_then(|id| DatabaseId::new(id).map_err(|error| error.to_string()))
        {
            Ok(database) => database,
            Err(reason) => {
                refuse(row, reason);
                continue;
            }
        };
        if let Err(error) =
            collaboration_central_core::read_fleet_copy_summary_in_scope(&mut scope, database).await
        {
            refuse(format!("{row} copy summary"), error.to_string());
        }
        if let Err(error) =
            collaboration_central_core::read_fleet_lease_projection_in_scope(&mut scope, database)
                .await
        {
            refuse(format!("{row} lease projection"), error.to_string());
        }
    }
    transaction.rollback().await?;
    Ok(refusals)
}

/// One Copy, as main's health and retention read it: fleet-sync-receiver's
/// `copy_status` and full `inspect_copy` (record, schema fingerprint, staged
/// parts); then `inspect_retained_records` (the `inspect` feature, enabled only
/// in this crate's dev-dependencies), which reads the creation request, fence
/// receipts, seed record, pages and attempts, install marker and page receipts,
/// and every staged part's payload through the receiver's own readers, and
/// whose per-family counts must equal the Copy's stored rows; then the
/// receiver's own row rules for the families it reads elsewhere: each
/// receipt's digest (`PieceDigest`) and each replicated table name
/// (`TableName`, outside the reserved namespace).
pub async fn decode_copy(
    project: &str,
    image: &ObjectImage,
    connection: &mut TursoConnection,
) -> Result<Vec<Refusal>, sqlx_core::Error> {
    let object = format!("copy {project}");
    let mut refusals = Vec::new();
    let mut refuse = |row: &str, reason: String| {
        refusals.push(Refusal {
            object: object.clone(),
            row: row.into(),
            reason,
        });
    };
    let mut transaction = connection.begin().await?;
    // As main's post-deploy Copy migration does before main reads it: the
    // pinned receiver corpus, recorded in central's own set (its checksum
    // check also validates the candidate's recorded versions), then the Copy's
    // receiver hook. All of it rolls back with the transaction.
    let mut receiver = sqlx_core::migrate::Migrator::with_migrations(
        fleet_sync_receiver::RECEIVER_MIGRATOR
            .iter()
            .cloned()
            .collect::<Vec<_>>(),
    );
    receiver.dangerous_set_table_name("_sqlx_migrations_fleet_sync_receiver");
    if let Err(error) = receiver.run_direct(None, &mut *transaction, false).await {
        refuse(
            "_sqlx_migrations_fleet_sync_receiver",
            format!("main's receiver migration refuses the Copy: {error}"),
        );
    }
    match collaboration_central_core::CopyReceiverRules::new(
        runtime_primitives::SyncTime::from_unix_millis(0),
        None,
    ) {
        Ok(rules) => {
            if let Err(error) =
                fleet_sync_receiver::initialize_hook(&mut TursoScope(&mut transaction), &rules)
                    .await
            {
                refuse(
                    "_fleet_sync_hook_migrations",
                    format!("main's receiver hook refuses the Copy: {error}"),
                );
            }
        }
        Err(error) => refuse("_fleet_sync_hook_migrations", error.to_string()),
    }
    let status = fleet_sync_receiver::copy_status(&mut TursoScope(&mut transaction)).await;
    let inspection = fleet_sync_receiver::inspect_copy(&mut TursoScope(&mut transaction)).await;
    let retained = fleet_sync_receiver::inspect_retained_records::<_, BoardAnnotation>(
        &mut TursoScope(&mut transaction),
    )
    .await;
    transaction.rollback().await?;
    match retained {
        Ok(records) => {
            for (family, decoded) in [
                (RetainedFamily::Creation, records.creation),
                (RetainedFamily::FenceReceipts, records.fence_receipts),
                (RetainedFamily::Seed, records.seed),
                (RetainedFamily::SeedPages, records.seed_pages),
                (RetainedFamily::SeedAttempts, records.seed_attempts),
                (RetainedFamily::Install, records.install),
                (RetainedFamily::InstallPages, records.install_pages),
                (RetainedFamily::StagedParts, records.staged_parts),
            ] {
                let stored = image.row_count(family.table());
                if decoded != stored {
                    refuse(
                        family.table(),
                        format!("main's receiver readers decode {decoded} of {stored} rows"),
                    );
                }
            }
        }
        Err(refusal) => refuse(
            refusal.family.table(),
            format!(
                "main's receiver reader refuses {}: {}",
                refusal.row, refusal.detail
            ),
        ),
    }
    match status {
        Ok(CopyStatusAnswer::Status(_) | CopyStatusAnswer::Retired) => {}
        Ok(CopyStatusAnswer::NotInitialized) => {
            refuse("_fleet_sync_copy", "main reads no Copy record".into())
        }
        Err(error) => refuse("_fleet_sync_copy", error.to_string()),
    }
    match inspection {
        fleet_sync_receiver::CopyInspection::Damaged { detail } => {
            refuse(
                "inspection",
                format!("main's Copy inspection marks it damaged: {detail}"),
            );
        }
        fleet_sync_receiver::CopyInspection::Uninitialized => {
            refuse("inspection", "main's Copy inspection finds no Copy".into());
        }
        fleet_sync_receiver::CopyInspection::Live { .. }
        | fleet_sync_receiver::CopyInspection::Retired { .. } => {}
    }
    for (index, identity, digest) in image.typed_pairs("_fleet_sync_receipts", "identity", "digest")
    {
        let row = format!("_fleet_sync_receipts row {index}");
        match (identity, digest) {
            (TypedValue::Text(_), TypedValue::Text(digest)) => {
                if fleet_sync_protocol::PieceDigest::try_from(digest.clone()).is_err() {
                    refuse(&row, "receipt digest isn't a piece digest".into());
                }
            }
            _ => refuse(&row, "receipt identity or digest isn't text".into()),
        }
    }
    for (index, name) in image.typed_column("_fleet_sync_copy_tables", "name") {
        let row = format!("_fleet_sync_copy_tables row {index}");
        match name {
            TypedValue::Text(name)
                if fleet_sync_protocol::TableName::new(name).is_ok()
                    && !fleet_sync_protocol::reserved_namespace(name) => {}
            _ => refuse(
                &row,
                "replicated schema membership isn't a valid table name".into(),
            ),
        }
    }
    Ok(refusals)
}
