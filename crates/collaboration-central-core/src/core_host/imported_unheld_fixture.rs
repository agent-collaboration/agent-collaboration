//! Conformance setup only: public import and seed on the actual composed objects.
use super::*;
use crate::{
    CallerContext, CopyKind, EdgeKind, ObjectHost, ObjectRuntime, OwnerIdentity, RegistryKind,
    copy_write_calls::{AdminCopySeedRequest, CopyAdminSeed},
    object_runtime::ObjectWork,
};
use durable_steps::{AtomicStore, OperationState, StoreError, storage::TransactionScope};
use fleet_sync_protocol::*;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

/// Parent board selected explicitly by a conformance import/seed caller.
#[derive(Clone, Copy, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ConformanceBoardState {
    Archived,
    Open,
}

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
    /// Does not exercise D1b's admin HTTP import workflow.
    pub async fn conformance_imported_unheld(
        &self,
        database: DatabaseId,
        owner: OwnerIdentity,
        board_state: ConformanceBoardState,
    ) -> Result<ReplicationPosition, StoreError> {
        let ObjectWork::Registry {
            engine,
            shared_host,
            config,
            ..
        } = &self.work
        else {
            return Err(StoreError::backend(
                "import fixture requires the composed Registry",
            ));
        };
        fixture_fleet(shared_host, &owner, database, board_state).await?;
        let attempt = RequestId::new(shared_host.new_id()).map_err(StoreError::backend)?;
        let operation = engine
            .start::<fleet_sync_lease::ImportTask<Host::Store, Host::Database>>(
                &fleet_sync_lease::lifecycle_request_key(attempt).map_err(StoreError::backend)?,
                Import {
                    attempt,
                    database,
                    digest: ImportDigest::new("d".repeat(64)).map_err(StoreError::backend)?,
                },
            )
            .await
            .map_err(StoreError::backend)?;
        let OperationState::Completed { result } = engine.status(&operation).await?.state else {
            return Err(StoreError::backend(
                "import fixture did not complete its real ImportTask",
            ));
        };
        if serde_json::from_slice::<ImportAnswer>(&result).map_err(StoreError::backend)?
            != ImportAnswer::Imported
        {
            return Err(StoreError::backend("import fixture was not newly imported"));
        }
        let target = CopyAdminSeed {
            copy: config.authoritative_receiver,
            database,
        };
        let seed_attempt = SeedAttemptId::new(shared_host.new_id()).map_err(StoreError::backend)?;
        let ReplicationAnswer::SeedStarted { seed } = fixture_seed(
            shared_host,
            &owner,
            &target,
            AdminCopySeedRequest::Begin {
                database,
                attempt: seed_attempt,
            },
        )
        .await?
        else {
            return Err(StoreError::backend("import fixture seed did not begin"));
        };
        let sql =
            "CREATE TABLE owner_retirement_rows(id TEXT NOT NULL PRIMARY KEY,value TEXT NOT NULL)";
        let schema = SchemaIdentity::new(
            1,
            schema_fingerprint(&[SchemaEntry {
                kind: SchemaObjectKind::Table,
                name: "owner_retirement_rows".into(),
                sql: sql.into(),
            }])
            .map_err(StoreError::backend)?,
        )
        .map_err(StoreError::backend)?;
        let row = vec![
            SqlValue::Text("019f0000-0000-7000-8000-000000005595".into()),
            SqlValue::Text("fixture imported row".into()),
        ];
        let mut digest = TableDigestBuilder::new();
        digest.push_row(&row).map_err(StoreError::backend)?;
        let position = ReplicationPosition::new(
            Epoch::new(1).map_err(StoreError::backend)?,
            CommitSequence::new(1),
        );
        let contents = SnapshotPage {
            database,
            position,
            schema: schema.clone(),
            schema_steps: vec![sql.into()],
            table: "owner_retirement_rows".into(),
            rows: vec![row],
            next: None,
        };
        let page = SeedPage {
            seed,
            page: 0,
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
            return Err(StoreError::backend(
                "import fixture seed page was not staged",
            ));
        }
        let done = SeedComplete {
            database,
            seed,
            position,
            pages: 1,
            schema,
            table_digests: vec![(
                TableName::new("owner_retirement_rows").map_err(StoreError::backend)?,
                digest.finish(),
            )],
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
            return Err(StoreError::backend("import fixture seed was not published"));
        }
        Ok(position)
    }
}
pub(super) async fn fixture_seed<Io: PlatformIo>(
    host: &CoreHost<Io>,
    owner: &OwnerIdentity,
    target: &CopyAdminSeed,
    request: AdminCopySeedRequest,
) -> Result<ReplicationAnswer, StoreError> {
    let caller = CallerContext::<EdgeKind>::for_owner(owner.clone());
    let call = crate::object_call_envelope::encode_shared_call::<EdgeKind, CopyKind, _>(
        &caller,
        target.database,
        CopyAdminSeed {
            copy: target.copy,
            database: target.database,
        },
        request,
        host.call_identity("imported_unheld_fixture_seed")
            .map_err(|failure| {
                StoreError::backend(format!("fixture seed identity: {failure:?}"))
            })?,
    )
    .map_err(|failure| StoreError::backend(format!("fixture seed encoding: {failure:?}")))?;
    let (address, bytes) = call.into_parts();
    let answer = host
        .io
        .object_call(address, bytes)
        .await
        .map_err(|failure| StoreError::backend(format!("fixture seed transport: {failure:?}")))?;
    serde_json::from_slice(&answer).map_err(StoreError::backend)
}

pub(super) async fn fixture_fleet<Io: PlatformIo>(
    host: &CoreHost<Io>,
    owner: &OwnerIdentity,
    database: DatabaseId,
    board_state: ConformanceBoardState,
) -> Result<(), StoreError> {
    use fleet_admission::{
        FleetImportBundle, FleetImportRecord, FleetImportResult, ImportProvenance,
        ImportedBoardState,
    };
    use message_board::{
        BoardId, Description, MachineLabel, ProjectId, RecordTime, ResourceName, ServiceId,
        WorkspaceId,
    };
    let workspace = WorkspaceId::try_from("019f0000-0000-7000-8000-000000005593".to_owned())
        .map_err(StoreError::backend)?;
    let board = BoardId::try_from("019f0000-0000-7000-8000-000000005594".to_owned())
        .map_err(StoreError::backend)?;
    let bundle = FleetImportBundle {
        declared_owner: message_board::HumanId::try_from("owner".to_owned())
            .map_err(StoreError::backend)?,
        provenance: ImportProvenance {
            installation: ServiceId::try_from("550e8400-e29b-41d4-a716-446655440595".to_owned())
                .map_err(StoreError::backend)?,
            machine: MachineLabel::try_from("conformance import fixture".to_owned())
                .map_err(StoreError::backend)?,
        },
        records: vec![
            FleetImportRecord::Workspace {
                id: workspace.clone(),
                name: ResourceName::try_from("Fixture imported workspace".to_owned())
                    .map_err(StoreError::backend)?,
                description: Description::try_from("conformance setup only".to_owned())
                    .map_err(StoreError::backend)?,
                repositories: vec![],
                time: RecordTime::LegacyUnknown,
            },
            FleetImportRecord::Board {
                id: board.clone(),
                workspace,
                name: ResourceName::try_from("Fixture imported board".to_owned())
                    .map_err(StoreError::backend)?,
                description: Description::try_from("conformance setup only".to_owned())
                    .map_err(StoreError::backend)?,
                state: match board_state {
                    ConformanceBoardState::Archived => ImportedBoardState::Archived,
                    ConformanceBoardState::Open => ImportedBoardState::Active,
                },
                time: RecordTime::LegacyUnknown,
            },
            FleetImportRecord::Directory {
                project: ProjectId::try_from(database.to_string()).map_err(StoreError::backend)?,
                board,
                title: ResourceName::try_from("Fixture imported project".to_owned())
                    .map_err(StoreError::backend)?,
                time: RecordTime::LegacyUnknown,
            },
        ],
    };
    let call = crate::serialize_object_call(
        &CallerContext::<EdgeKind>::for_owner(owner.clone()),
        crate::Dispatch::<crate::FleetKind> {
            call_id: crate::ObjectCallId::new(host.new_id().to_string())
                .map_err(StoreError::backend)?,
            to: (),
            call: crate::FleetCall::Import {
                bundle: Box::new(bundle),
            },
        },
    )
    .map_err(|failure| StoreError::backend(format!("fixture Fleet encoding: {failure:?}")))?;
    let (address, bytes) = call.into_parts();
    let answer = host
        .io
        .object_call(address, bytes)
        .await
        .map_err(|failure| StoreError::backend(format!("fixture Fleet transport: {failure:?}")))?;
    let crate::FleetResult::Imported(FleetImportResult::Imported { .. }) =
        serde_json::from_slice(&answer).map_err(StoreError::backend)?
    else {
        return Err(StoreError::backend(
            "fixture canonical Fleet graph was not imported",
        ));
    };
    Ok(())
}
