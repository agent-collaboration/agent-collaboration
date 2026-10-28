//! Conformance-only import of genuine consumer snapshots through the owning engines.
use super::imported_unheld_fixture::{fixture_fleet, fixture_seed};
use super::{IdSource, PlatformIo};
use crate::{
    ConformanceBoardState, ObjectHost, ObjectRuntime, OwnerIdentity, RegistryKind,
    copy_write_calls::{AdminCopySeedRequest, CopyAdminSeed},
    object_runtime::ObjectWork,
};
use durable_steps::{AtomicStore, OperationState, StoreError, storage::TransactionScope};
use fleet_sync_protocol::{
    Import, ImportAnswer, ImportDigest, PageDigest, ReplicationAnswer, ReplicationPosition,
    RequestId, SeedAnswer, SeedAttemptId, SeedComplete, SeedPage, SeedPageAnswer, SnapshotManifest,
    SnapshotPage,
};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

impl<Host: ObjectHost, Io: PlatformIo> ObjectRuntime<RegistryKind, Host, Io>
where
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    /// No HTTP route exposes this. ImportTask and admin seeding remain real;
    /// the fixture supplies only the already-captured consumer snapshot.
    pub async fn conformance_import_board_snapshot(
        &self,
        owner: OwnerIdentity,
        board_state: ConformanceBoardState,
        pages: Vec<SnapshotPage>,
        manifest: SnapshotManifest,
    ) -> Result<ReplicationPosition, StoreError> {
        let ObjectWork::Registry {
            engine,
            shared_host,
            config,
            ..
        } = &self.work
        else {
            return Err(StoreError::backend("board import needs Registry"));
        };
        let database = manifest.database;
        fixture_fleet(shared_host, &owner, database, board_state).await?;
        let attempt = RequestId::new(shared_host.new_id()).map_err(StoreError::backend)?;
        let operation = engine
            .start::<fleet_sync_lease::ImportTask<Host::Store, Host::Database>>(
                &fleet_sync_lease::lifecycle_request_key(attempt).map_err(StoreError::backend)?,
                Import {
                    attempt,
                    database,
                    digest: ImportDigest::new("e".repeat(64)).map_err(StoreError::backend)?,
                },
            )
            .await
            .map_err(StoreError::backend)?;
        let OperationState::Completed { result } = engine.status(&operation).await?.state else {
            return Err(StoreError::backend("board import task is not Completed"));
        };
        if serde_json::from_slice::<ImportAnswer>(&result).map_err(StoreError::backend)?
            != ImportAnswer::Imported
        {
            return Err(StoreError::backend("board import was not imported"));
        }
        let target = CopyAdminSeed {
            copy: config.authoritative_receiver,
            database,
        };
        let ReplicationAnswer::SeedStarted { seed } = fixture_seed(
            shared_host,
            &owner,
            &target,
            AdminCopySeedRequest::Begin {
                database,
                attempt: SeedAttemptId::new(shared_host.new_id()).map_err(StoreError::backend)?,
            },
        )
        .await?
        else {
            return Err(StoreError::backend("board admin seed did not start"));
        };
        let count = u32::try_from(pages.len()).map_err(StoreError::backend)?;
        for (index, contents) in pages.into_iter().enumerate() {
            let page = SeedPage {
                seed,
                page: u32::try_from(index).map_err(StoreError::backend)?,
                digest: PageDigest::of(&contents).map_err(StoreError::backend)?,
                contents,
            };
            if fixture_seed(
                shared_host,
                &owner,
                &target,
                AdminCopySeedRequest::Page(Box::new(page)),
            )
            .await?
                != ReplicationAnswer::SeedPage(SeedPageAnswer::Staged)
            {
                return Err(StoreError::backend("board admin seed page refused"));
            }
        }
        let position = manifest.position;
        let done = SeedComplete {
            database,
            seed,
            position,
            pages: count,
            schema: manifest.schema,
            table_digests: manifest
                .tables
                .into_iter()
                .map(|table| (table.table, table.digest))
                .collect(),
        };
        if fixture_seed(
            shared_host,
            &owner,
            &target,
            AdminCopySeedRequest::Complete {
                done,
                summary: None,
            },
        )
        .await?
            != ReplicationAnswer::SeedComplete(SeedAnswer::Published { position })
        {
            return Err(StoreError::backend("board admin seed did not publish"));
        }
        Ok(position)
    }
}
