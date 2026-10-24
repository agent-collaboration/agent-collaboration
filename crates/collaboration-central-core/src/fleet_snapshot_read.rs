//! Snapshot pages reuse each canonical record owner's validated decoding.
use crate::{
    fleet_record_codec::{corrupt, required},
    fleet_record_store, generated_fleet_record_reads as queries,
};
use durable_steps::{StoreError, storage::TransactionScope};
use message_board::{Record, RecordId, RecordKind, RecordRef};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub(crate) async fn records<Scope: TransactionScope>(
    scope: &mut Scope,
    after: Option<RecordRef>,
    limit: usize,
) -> Result<(Vec<Record>, Option<RecordRef>), StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    if limit == 0 {
        return Err(corrupt("snapshot.limit", "page limit must be positive"));
    }
    let (after_id, after_kind) = match &after {
        Some(reference) => (
            reference.id.as_str().to_owned(),
            crate::fleet_record_codec::kind_tag(reference.kind)?.to_owned(),
        ),
        None => (String::new(), String::new()),
    };
    let lookahead = limit
        .checked_add(1)
        .ok_or_else(|| corrupt("snapshot.limit", "page limit overflow"))?;
    // Workerd permits five compound SELECT terms. A flat read per record owner
    // keeps every statement below that limit, independent of the record count.
    // Each owner contributes at most one page plus lookahead; merge before
    // truncating so kind never outranks the global record identity order.
    let mut rows = Vec::new();
    macro_rules! append_owner_keys {
        ($read:ident) => {
            rows.extend(
                queries::$read(
                    scope,
                    after_id.clone(),
                    after_kind.clone(),
                    lookahead.to_string(),
                )
                .await
                .map_err(StoreError::backend)?,
            );
        };
    }
    append_owner_keys!(read_workspace_snapshot_keys);
    append_owner_keys!(read_board_snapshot_keys);
    append_owner_keys!(read_project_snapshot_keys);
    append_owner_keys!(read_adr_snapshot_keys);
    append_owner_keys!(read_process_definition_snapshot_keys);
    append_owner_keys!(read_request_snapshot_keys);
    rows.sort_by(|left, right| (&left.record_id, &left.kind).cmp(&(&right.record_id, &right.kind)));
    rows.truncate(lookahead);
    let mut keys = Vec::with_capacity(rows.len());
    for row in rows {
        let raw = required(row.record_id, "snapshot.id")?;
        let id = RecordId::try_from(raw.clone()).map_err(|error| corrupt("snapshot.id", error))?;
        if id.as_str() != raw {
            return Err(corrupt("snapshot.id", "record key would be normalized"));
        }
        let kind = match required(row.kind, "snapshot.kind")?.as_str() {
            "workspace" => RecordKind::Workspace,
            "board" => RecordKind::Board,
            "project" => RecordKind::Project,
            "adr" => RecordKind::Adr,
            "processDefinition" => RecordKind::ProcessDefinition,
            "request" => RecordKind::Request,
            _ => {
                return Err(corrupt(
                    "snapshot.kind",
                    "record kind is not owned by Fleet",
                ));
            }
        };
        keys.push(RecordRef { kind, id });
    }
    let has_more = keys.len() > limit;
    if has_more {
        keys.pop();
    }
    let mut records = Vec::with_capacity(keys.len());
    for reference in &keys {
        let stored = fleet_record_store::read(scope, reference)
            .await?
            .ok_or_else(|| {
                corrupt(
                    "snapshot.record",
                    "key has no canonical record in this transaction",
                )
            })?;
        records.push(stored.record);
    }
    let next = if has_more { keys.last().cloned() } else { None };
    Ok((records, next))
}
