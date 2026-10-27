//! Bounded controls over the real bootstrap, library rules and store boundary.

use super::{
    AtomicStore, BoxFuture, ColumnIndex, Database, Decode, Deserialize, Encode, EngineSchema,
    Executor, Infallible, IntoArguments, ObjectBootstrap, ObjectHost, Serialize, StepCallback,
    StepResult, StoreError, TransactionScope, Type, corrupt,
};
use crate::generated_bootstrap_probes as queries;
use board_sync_protocol::BoardDeliveryTypes;
use fleet_sync_protocol::{
    AttachSequence, CopyOrigin, CopyStatusAnswer, CreateCopy, DatabaseId, DeliveryTypes, Epoch,
    ItemId, PeerId, QueryResult, ReplicationAnswer, ReportSchemaSupport, SchemaSupport, SessionId,
};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HistoryCorruption {
    Checksum,
    NoncanonicalVersion,
    UnknownVersion,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BootstrapProbe {
    Registry,
    Copy,
    Mailbox,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum BootstrapCommand {
    Initialize {
        schema: EngineSchema,
    },
    Snapshot,
    PrepareLateConflict,
    CorruptHistory {
        schema: EngineSchema,
        corruption: HistoryCorruption,
    },
    Probe {
        schema: BootstrapProbe,
    },
    CopyStatus {
        database: DatabaseId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BootstrapHistory {
    pub corpus: String,
    pub version: String,
    pub checksum: String,
}

type Acceptance = <BoardDeliveryTypes as DeliveryTypes>::Acceptance;
#[derive(Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum BootstrapProbeReply {
    Registry(ReportSchemaSupport),
    Copy(Box<CopyStatusAnswer>),
    Mailbox(QueryResult<Acceptance>),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum BootstrapReply {
    Initialized {
        applied: usize,
    },
    Snapshot {
        tables: Vec<String>,
        history: Vec<BootstrapHistory>,
    },
    ConflictPrepared,
    HistoryCorrupted,
    Probed(Box<BootstrapProbeReply>),
    CopyStatus(Box<ReplicationAnswer>),
}

impl<Host: ObjectHost> ObjectBootstrap<Host>
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
    pub async fn execute(&self, command: BootstrapCommand) -> Result<BootstrapReply, StoreError> {
        match command {
            BootstrapCommand::Initialize { schema } => Ok(BootstrapReply::Initialized {
                applied: self.initialize(schema).await?,
            }),
            BootstrapCommand::CopyStatus { database } => {
                let answer = match self.driver.read_copy_status(database).await {
                    Ok(status) => ReplicationAnswer::CopyStatus(status),
                    Err(failure) => failure.answer(),
                };
                Ok(BootstrapReply::CopyStatus(Box::new(answer)))
            }
            command => {
                let (reply, _effects) = self.driver.run(BootstrapProbeStep { command }).await?;
                Ok(reply)
            }
        }
    }
}

struct BootstrapProbeStep {
    command: BootstrapCommand,
}
impl<Store, Driver> StepCallback<Store, Driver> for BootstrapProbeStep
where
    Store: AtomicStore,
    Driver: Database,
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = BootstrapReply;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            let reply = match self.command {
                BootstrapCommand::Snapshot => {
                    let tables = queries::bootstrap_probe_tables(scope)
                        .await
                        .map_err(StoreError::backend)?
                        .into_iter()
                        .map(|row| row.name.ok_or_else(|| corrupt("missing table name")))
                        .collect::<Result<Vec<_>, _>>()?;
                    let mut history = Vec::new();
                    if tables
                        .iter()
                        .any(|name| name == "central_schema_migrations")
                    {
                        for row in queries::bootstrap_probe_history(scope)
                            .await
                            .map_err(StoreError::backend)?
                        {
                            history.push(BootstrapHistory {
                                corpus: row.corpus.ok_or_else(|| corrupt("missing corpus"))?,
                                version: row.version.ok_or_else(|| corrupt("missing version"))?,
                                checksum: row
                                    .checksum
                                    .ok_or_else(|| corrupt("missing checksum"))?,
                            });
                        }
                    }
                    BootstrapReply::Snapshot { tables, history }
                }
                BootstrapCommand::PrepareLateConflict => {
                    // This migration has only two independent CREATE statements.
                    // Bootstrap subsequently writes migrations 1 and 2 before
                    // the third meets these already-existing tables.
                    let migration = fleet_sync_receiver::RECEIVER_MIGRATOR
                        .iter()
                        .find(|migration| migration.version == 3)
                        .ok_or_else(|| corrupt("snapshot-install migration absent"))?;
                    scope
                        .executor()
                        .execute(migration.sql.as_ref())
                        .await
                        .map_err(StoreError::backend)?;
                    BootstrapReply::ConflictPrepared
                }
                BootstrapCommand::CorruptHistory { schema, corruption } => {
                    match corruption {
                        HistoryCorruption::Checksum => {
                            queries::bootstrap_probe_checksum(
                                scope,
                                "[]".into(),
                                schema.corpus().into(),
                            )
                            .await
                        }
                        HistoryCorruption::NoncanonicalVersion => {
                            queries::bootstrap_probe_version(
                                scope,
                                "01".into(),
                                schema.corpus().into(),
                            )
                            .await
                        }
                        HistoryCorruption::UnknownVersion => {
                            queries::bootstrap_probe_version(
                                scope,
                                "999".into(),
                                schema.corpus().into(),
                            )
                            .await
                        }
                    }
                    .map_err(StoreError::backend)?;
                    BootstrapReply::HistoryCorrupted
                }
                BootstrapCommand::Probe { schema } => {
                    let peer = PeerId::try_from("019f0000-0000-7000-8000-000000000601".to_owned())
                        .map_err(StoreError::backend)?;
                    let probe = match schema {
                        BootstrapProbe::Registry => {
                            let report = ReportSchemaSupport {
                                peer,
                                session_id: SessionId::try_from(
                                    "019f0000-0000-7000-8000-000000000602".to_owned(),
                                )
                                .map_err(StoreError::backend)?,
                                attach_seq: AttachSequence::new(1).map_err(StoreError::backend)?,
                                support: SchemaSupport { max_version: 7 },
                            };
                            fleet_sync_lease::report_schema_support(scope, &report)
                                .await
                                .map_err(StoreError::backend)?;
                            let stored = fleet_sync_lease::accepted_schema_support(scope, peer)
                                .await
                                .map_err(StoreError::backend)?
                                .ok_or_else(|| corrupt("schema report not stored"))?;
                            BootstrapProbeReply::Registry(stored)
                        }
                        BootstrapProbe::Copy => {
                            let database = DatabaseId::try_from(
                                "019f0000-0000-7000-8000-000000000603".to_owned(),
                            )
                            .map_err(StoreError::backend)?;
                            let request = CreateCopy {
                                database,
                                epoch: Epoch::new(1).map_err(StoreError::backend)?,
                                holder: Some(peer),
                                origin: CopyOrigin::Empty,
                            };
                            let _answer = fleet_sync_receiver::create_copy(scope, &request)
                                .await
                                .map_err(fleet_sync_receiver::receiver_rollback)?;
                            BootstrapProbeReply::Copy(Box::new(
                                fleet_sync_receiver::copy_status(scope)
                                    .await
                                    .map_err(fleet_sync_receiver::receiver_rollback)?,
                            ))
                        }
                        BootstrapProbe::Mailbox => {
                            let allowance = fleet_sync_delivery::minimum_settlement_allowance::<
                                BoardDeliveryTypes,
                            >()
                            .map_err(StoreError::backend)?;
                            let limits =
                                fleet_sync_delivery::DeliveryLimits::new::<BoardDeliveryTypes>(
                                    1_000_000, 100, 1_000_000, 100, 1_000_000, allowance,
                                )
                                .map_err(StoreError::backend)?;
                            let item =
                                ItemId::try_from("019f0000-0000-7000-8000-000000000604".to_owned())
                                    .map_err(StoreError::backend)?;
                            let result =
                                fleet_sync_delivery::Mailbox::<_, BoardDeliveryTypes>::new(
                                    scope, peer, limits,
                                )
                                .query(&[item])
                                .await
                                .map_err(StoreError::backend)?;
                            BootstrapProbeReply::Mailbox(result)
                        }
                    };
                    BootstrapReply::Probed(Box::new(probe))
                }
                BootstrapCommand::Initialize { .. } | BootstrapCommand::CopyStatus { .. } => {
                    return Err(corrupt("probe command reached the wrong entry"));
                }
            };
            Ok((reply, Vec::new()))
        })
    }
}
