//! A mutation's notices are appended in the mutation's own transaction; the
//! Fleet's notice push sends them after it commits (no engine job).
use crate::{fleet_feed_readers, fleet_feed_store};
use board_sync_protocol::BoardNotice;
use durable_steps::{StoreError, storage::TransactionScope};
use message_board::FleetPosition;
use runtime_primitives::SyncTime;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub(crate) async fn admit_mutation_notices<
    Driver: Database,
    Scope: TransactionScope<Database = Driver>,
>(
    scope: &mut Scope,
    previous: FleetPosition,
    now: SyncTime,
    row_ids: &mut crate::fleet_row_ids::FleetRowIds,
) -> Result<(), StoreError>
where
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Decode<'value, Driver> + Encode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    let head = fleet_feed_store::high_water(scope).await?;
    if head == previous {
        return Ok(());
    }
    if head < previous {
        return Err(crate::fleet_record_codec::corrupt(
            "feed.head",
            "mutation regressed the feed",
        ));
    }
    for entry in fleet_feed_store::read_range(scope, previous, head).await? {
        let message_board::Record::Board(board) = entry.record else {
            continue;
        };
        if board.state != message_board::BoardState::Archiving {
            continue;
        }
        for installation in holder_installations(scope, &board.board_id, now).await? {
            append_for(scope, &installation, now, row_ids).await?;
        }
    }
    for reader in fleet_feed_readers::readers(scope, head).await? {
        if reader.applied >= head {
            continue;
        }
        append_for(scope, &reader.installation, now, row_ids).await?;
    }
    Ok(())
}

/// Append a project-list notice to the stream of the machine enrolled for
/// `installation`. An installation with no single enrollment gets no notice.
async fn append_for<Scope: TransactionScope>(
    scope: &mut Scope,
    installation: &message_board::ServiceId,
    now: SyncTime,
    row_ids: &mut crate::fleet_row_ids::FleetRowIds,
) -> Result<(), StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>:
        Type<Scope::Database> + Decode<'value, Scope::Database> + Encode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let mut enrolled = crate::generated_enrollment::read_enrollments(
        scope,
        Some(installation.as_str().to_owned()),
        None,
        None,
        None,
    )
    .await
    .map_err(StoreError::backend)?;
    if enrolled.len() != 1 {
        eprintln!(
            "Fleet notice skipped: installation {} has no single enrollment",
            installation.as_str()
        );
        return Ok(());
    }
    let identity = crate::machine_enrollment_record::identity_from_stored_enrollment(
        enrolled
            .pop()
            .ok_or_else(|| StoreError::backend("missing notice recipient"))?,
    )?;
    crate::machine_notice_store::append(
        scope,
        identity.peer,
        row_ids.next()?.to_string(),
        BoardNotice::ProjectListChanged {},
        now,
    )
    .await
    .map(|_| ())
}

async fn holder_installations<Scope: TransactionScope>(
    scope: &mut Scope,
    board: &message_board::BoardId,
    now: SyncTime,
) -> Result<Vec<message_board::ServiceId>, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>:
        Type<Scope::Database> + Decode<'value, Scope::Database> + Encode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    use crate::fleet_record_codec::{corrupt, required};
    let mut snapshot = fleet_admission::FleetSnapshot::empty();
    crate::fleet_archive_snapshot::board_projects(scope, &mut snapshot, board).await?;
    let mut installations = Vec::new();
    for project in snapshot.board_projects {
        let row =
            crate::generated_fleet_lease::read_directory_lease(scope, project.id.as_str().into())
                .await
                .map_err(StoreError::backend)?
                .ok_or_else(|| corrupt("archive.notice", "directory lease disappeared"))?;
        if required(row.project_id.clone(), "archive.notice.project")? != project.id.as_str() {
            return Err(corrupt(
                "archive.notice.project",
                "lookup belongs to another project",
            ));
        }
        if let Some(projection) = crate::fleet_lease_columns::decode(row)? {
            if projection.database().as_uuid().to_string() != project.id.as_str() {
                return Err(corrupt(
                    "archive.notice.lease",
                    "projection belongs to another project",
                ));
            }
            if let fleet_admission::DirectoryLease::Held { holder, .. } =
                projection.directory_metadata(now)?.lease
                && !installations.contains(&holder)
            {
                installations.push(holder);
            }
        }
    }
    Ok(installations)
}
