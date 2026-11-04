//! Typed feed writes and batched child reads stay in the caller's existing transaction.
use crate::{
    fleet_column_codec::{ActorColumns, canonical, optional_text, text},
    fleet_feed_snapshot::SnapshotColumns,
    fleet_record_codec::{FleetRecordHeader, corrupt},
    generated_fleet_feed as feed, generated_fleet_feed_lists as lists,
};
use durable_steps::{StoreError, storage::TransactionScope};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::collections::BTreeMap;

pub(crate) struct FeedLists {
    repositories: BTreeMap<String, Vec<lists::StoredFeedRepositoryRef>>,
    files: BTreeMap<String, Vec<lists::StoredFeedAdrFile>>,
    pub requests: BTreeMap<message_board::RequestId, message_board::RequestLedgerEntry>,
}
impl FeedLists {
    fn new(
        repositories: Vec<lists::StoredFeedRepositoryRef>,
        files: Vec<lists::StoredFeedAdrFile>,
        requests: BTreeMap<message_board::RequestId, message_board::RequestLedgerEntry>,
    ) -> Result<Self, StoreError> {
        let mut grouped = Self {
            repositories: BTreeMap::new(),
            files: BTreeMap::new(),
            requests,
        };
        for row in repositories {
            let parent = text(
                row.entry_id.clone(),
                row.entry_id_class.clone(),
                "feed.repository.parent",
            )?;
            crate::fleet_row_codec::check_private_id(&parent, "feed.repository.parent")?;
            grouped.repositories.entry(parent).or_default().push(row);
        }
        for row in files {
            let parent = text(
                row.entry_id.clone(),
                row.entry_id_class.clone(),
                "feed.file.parent",
            )?;
            crate::fleet_row_codec::check_private_id(&parent, "feed.file.parent")?;
            grouped.files.entry(parent).or_default().push(row);
        }
        Ok(grouped)
    }
    pub(crate) fn take(
        &mut self,
        parent: &str,
    ) -> (
        Vec<lists::StoredFeedRepositoryRef>,
        Vec<lists::StoredFeedAdrFile>,
    ) {
        (
            self.repositories.remove(parent).unwrap_or_default(),
            self.files.remove(parent).unwrap_or_default(),
        )
    }
    pub(crate) fn finish(self) -> Result<(), StoreError> {
        if !self.repositories.is_empty() || !self.files.is_empty() || !self.requests.is_empty() {
            return Err(corrupt(
                "feed.children",
                "queried children have no parent in the same range",
            ));
        }
        Ok(())
    }
}
pub(crate) async fn read_range<S: TransactionScope>(
    scope: &mut S,
    after: String,
    count: String,
    entries: &[feed::StoredFleetFeedEntry],
) -> Result<FeedLists, StoreError>
where
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'v> String: Type<S::Database> + Encode<'v, S::Database>,
    for<'v> Option<String>: Type<S::Database> + Decode<'v, S::Database>,
    for<'r> &'r str: ColumnIndex<<S::Database as Database>::Row>,
{
    let repositories = lists::read_feed_repositories_page(scope, after.clone(), count.clone())
        .await
        .map_err(StoreError::backend)?;
    let files = lists::read_feed_adr_files_page(scope, after, count)
        .await
        .map_err(StoreError::backend)?;
    let requests = read_request_ledgers(scope, entries).await?;
    FeedLists::new(repositories, files, requests)
}
pub(crate) async fn read_expired<S: TransactionScope>(
    scope: &mut S,
    cutoff: String,
    entries: &[feed::StoredFleetFeedEntry],
) -> Result<FeedLists, StoreError>
where
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'v> String: Type<S::Database> + Encode<'v, S::Database>,
    for<'v> Option<String>: Type<S::Database> + Decode<'v, S::Database>,
    for<'r> &'r str: ColumnIndex<<S::Database as Database>::Row>,
{
    let repositories = lists::read_expired_feed_repositories(scope, cutoff.clone())
        .await
        .map_err(StoreError::backend)?;
    let files = lists::read_expired_feed_adr_files(scope, cutoff)
        .await
        .map_err(StoreError::backend)?;
    let requests = read_request_ledgers(scope, entries).await?;
    FeedLists::new(repositories, files, requests)
}

async fn read_request_ledgers<S: TransactionScope>(
    scope: &mut S,
    entries: &[feed::StoredFleetFeedEntry],
) -> Result<BTreeMap<message_board::RequestId, message_board::RequestLedgerEntry>, StoreError>
where
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'v> String: Type<S::Database> + Encode<'v, S::Database>,
    for<'v> Option<String>: Type<S::Database> + Decode<'v, S::Database>,
    for<'r> &'r str: ColumnIndex<<S::Database as Database>::Row>,
{
    let mut requests = Vec::new();
    for row in entries {
        if let Some(id) = optional_text(
            row.ledger_request_id.clone(),
            row.ledger_request_id_class.clone(),
            "feed.ledger_request",
        )? {
            requests.push(canonical(id, "feed.ledger_request")?);
        }
    }
    crate::fleet_decision_step::read_ledgers(scope, &requests).await
}

pub(crate) async fn append<S: TransactionScope>(
    scope: &mut S,
    entry: &fleet_admission::FleetFeedEntry,
    entry_id: durable_steps::Uuid,
    now: chrono::DateTime<chrono::Utc>,
    row_ids: &mut crate::fleet_row_ids::FleetRowIds,
) -> Result<(), StoreError>
where
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'v> String: Type<S::Database> + Encode<'v, S::Database>,
    for<'v> Option<String>: Type<S::Database> + Encode<'v, S::Database>,
{
    crate::fleet_row_ids::validate_row_id(entry_id)?;
    let envelope = entry.record.envelope();
    let header = FleetRecordHeader::for_record(&entry.record)?;
    let actor = ActorColumns::encode(&envelope.actor);
    let body = SnapshotColumns::encode(&entry.record)?;
    let reference = entry.record.reference();
    feed::append_fleet_feed(
        scope,
        entry_id.to_string(),
        entry.position.get().to_string(),
        crate::fleet_record_codec::kind_tag(reference.kind)?.into(),
        reference.id.as_str().into(),
        envelope.scope.workspace.as_str().into(),
        envelope.scope.board.as_ref().map(|id| id.as_str().into()),
        actor.kind,
        actor.human,
        actor.service,
        actor.endpoint,
        actor.session,
        actor.component,
        envelope.request.as_ref().map(|id| id.as_str().into()),
        header.router_time(),
        header.time_basis(),
        header.origin.installation.as_str().into(),
        header.origin.machine.as_str().into(),
        now.timestamp_millis().to_string(),
        crate::fleet_feed_header::timestamp(now),
        body.name,
        body.description,
        body.workspace_state,
        body.repository_count.map(|count| count.to_string()),
        body.board_kind,
        body.initiative_goal,
        body.initiative_done_when,
        body.board_state,
        body.project_title,
        body.adr_title,
        body.adr_text,
        body.adr_status,
        body.supersedes_adr_id,
        body.made_in_kind,
        body.made_in_record_id,
        body.adr_revision,
        body.repository_file_count.map(|count| count.to_string()),
        body.ledger_request_id,
    )
    .await
    .map_err(StoreError::backend)?;
    match &entry.record {
        message_board::Record::Workspace(workspace) => {
            for (ordinal, reference) in workspace.repositories.iter().enumerate() {
                let (kind, origin, service, directory) = match reference {
                    message_board::RepositoryRef::Origin { normalized_origin } => (
                        "origin",
                        Some(normalized_origin.as_str().into()),
                        None,
                        None,
                    ),
                    message_board::RepositoryRef::Local {
                        service_id,
                        common_directory,
                    } => (
                        "local",
                        None,
                        Some(service_id.as_str().into()),
                        Some(common_directory.as_str().into()),
                    ),
                };
                lists::insert_feed_repositories(
                    scope,
                    row_ids.next()?.to_string(),
                    entry_id.to_string(),
                    ordinal.to_string(),
                    kind.into(),
                    origin,
                    service,
                    directory,
                    now.timestamp_millis().to_string(),
                )
                .await
                .map_err(StoreError::backend)?;
            }
        }
        message_board::Record::Adr(adr) => {
            for (ordinal, file) in adr.repository_files.iter().enumerate() {
                lists::insert_feed_adr_files(
                    scope,
                    row_ids.next()?.to_string(),
                    entry_id.to_string(),
                    ordinal.to_string(),
                    file.repository.as_str().into(),
                    file.path.clone(),
                    file.commit.as_ref().map(|commit| commit.as_str().into()),
                    now.timestamp_millis().to_string(),
                )
                .await
                .map_err(StoreError::backend)?;
            }
        }
        _ => {}
    }
    Ok(())
}
