//! Notice jobs commit with the mutation; no delivery is performed in this scope.
use crate::{
    fleet_feed_readers, fleet_feed_store,
    machine_notice_outbox::{FleetNoticeTask, MachineNoticeInput},
};
use board_sync_protocol::BoardNotice;
use durable_steps::{AtomicStore, JournalScope, RequestKey, StoreError, storage::TransactionScope};
use message_board::FleetPosition;
use runtime_primitives::SyncTime;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub(crate) async fn admit_mutation_notices<Store: AtomicStore, Driver: Database>(
    scope: &mut Store::Scope<'_>,
    previous: FleetPosition,
    now: SyncTime,
) -> Result<(), StoreError>
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Decode<'value, Driver>,
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
            admit::<Store>(
                scope,
                format!(
                    "fleet-archive-notice:{}:{}:{}",
                    entry.position.get(),
                    board.board_id.as_str(),
                    installation.as_str()
                ),
                MachineNoticeInput {
                    installation,
                    notice: BoardNotice::BoardArchiving {
                        board: board.board_id.clone(),
                    },
                },
                now,
            )
            .await?;
        }
    }
    for reader in fleet_feed_readers::readers(scope, head).await? {
        if reader.applied >= head {
            continue;
        }
        let key = RequestKey::new(format!(
            "fleet-feed-notice:{}:{}",
            head.get(),
            reader.installation.as_str()
        ))
        .map_err(StoreError::backend)?;
        scope
            .admit::<FleetNoticeTask<Store>>(
                &key,
                &MachineNoticeInput {
                    installation: reader.installation,
                    notice: BoardNotice::FleetFeedAdvanced { through: head },
                },
            )
            .await
            .map_err(StoreError::backend)?;
        scope.ensure_wake(now).await?;
    }
    Ok(())
}

async fn admit<Store: AtomicStore>(
    scope: &mut Store::Scope<'_>,
    key: String,
    input: MachineNoticeInput,
    now: SyncTime,
) -> Result<(), StoreError> {
    scope
        .admit::<FleetNoticeTask<Store>>(
            &RequestKey::new(key).map_err(StoreError::backend)?,
            &input,
        )
        .await
        .map_err(StoreError::backend)?;
    scope.ensure_wake(now).await
}

async fn holder_installations<Scope: TransactionScope>(
    scope: &mut Scope,
    board: &message_board::BoardId,
    now: SyncTime,
) -> Result<Vec<message_board::ServiceId>, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    use crate::fleet_record_codec::{corrupt, decode_json, required};
    let mut snapshot = fleet_admission::FleetSnapshot::empty();
    crate::fleet_archive_snapshot::board_projects(scope, &mut snapshot, board).await?;
    let mut installations = Vec::new();
    for project in snapshot.board_projects {
        let row =
            crate::generated_fleet_lease::read_directory_lease(scope, project.id.as_str().into())
                .await
                .map_err(StoreError::backend)?
                .ok_or_else(|| corrupt("archive.notice", "directory lease disappeared"))?;
        if required(row.project_id, "archive.notice.project")? != project.id.as_str() {
            return Err(corrupt(
                "archive.notice.project",
                "lookup belongs to another project",
            ));
        }
        if let Some(payload) = row.lease_projection {
            let projection: crate::lease_projection::LeaseProjection =
                decode_json(&payload, "archive.notice.lease")?;
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
