//! Bounded controls over the real bootstrap, library rules and store boundary.

use super::{
    AtomicStore, BoxFuture, ColumnIndex, Database, Decode, Deserialize, Encode, EngineSchema,
    Executor, Infallible, IntoArguments, ObjectBootstrap, ObjectPlatform, Serialize, StepCallback,
    StepResult, StoreError, TransactionScope, Type, corrupt,
};
use crate::generated_bootstrap_probes as queries;
use board_sync_protocol::BoardDeliveryTypes;
use fleet_protocol::{
    AttachSequence, CopyOrigin, CopyStatusAnswer, CreateCopy, DatabaseId, DeliveryTypes, Epoch,
    ItemId, PeerId, QueryResult, ReplicationAnswer, ReportSchemaSupport, SchemaSupport, SessionId,
};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HistoryCorruption {
    Checksum,
    InvalidVersion,
    UnknownVersion,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BootstrapProbe {
    Registry,
    Copy,
    Delivery,
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
    pub version: i64,
    pub description: String,
    pub installed_on: i64,
    pub success: i64,
    pub execution_time: i64,
    pub checksum: Vec<u8>,
    pub version_class: String,
    pub checksum_class: String,
    pub installed_class: String,
    pub execution_class: String,
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
    Delivery(QueryResult<Acceptance>),
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

impl<Host: ObjectPlatform> ObjectBootstrap<Host>
where
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'blob> Vec<u8>: sqlx_core::types::Type<Host::Database>
        + sqlx_core::encode::Encode<'blob, Host::Database>
        + sqlx_core::decode::Decode<'blob, Host::Database>,
    for<'scalar> i64: sqlx_core::types::Type<Host::Database>
        + sqlx_core::encode::Encode<'scalar, Host::Database>
        + sqlx_core::decode::Decode<'scalar, Host::Database>,
    for<'blob> Option<Vec<u8>>:
        sqlx_core::types::Type<Host::Database> + sqlx_core::decode::Decode<'blob, Host::Database>,
    for<'scalar> Option<i64>:
        sqlx_core::types::Type<Host::Database> + sqlx_core::decode::Decode<'scalar, Host::Database>,
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
                let (reply, _effects) = self
                    .driver
                    .run(BootstrapProbeStep {
                        command,
                        now: self.now,
                    })
                    .await?;
                Ok(reply)
            }
        }
    }
}

struct BootstrapProbeStep {
    command: BootstrapCommand,
    now: runtime_primitives::SyncTime,
}
impl<Store, Driver> StepCallback<Store, Driver> for BootstrapProbeStep
where
    Store: AtomicStore,
    Driver: Database,
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'blob> Vec<u8>: sqlx_core::types::Type<Driver>
        + sqlx_core::encode::Encode<'blob, Driver>
        + sqlx_core::decode::Decode<'blob, Driver>,
    for<'scalar> i64: sqlx_core::types::Type<Driver>
        + sqlx_core::encode::Encode<'scalar, Driver>
        + sqlx_core::decode::Decode<'scalar, Driver>,
    for<'blob> Option<Vec<u8>>:
        sqlx_core::types::Type<Driver> + sqlx_core::decode::Decode<'blob, Driver>,
    for<'scalar> Option<i64>:
        sqlx_core::types::Type<Driver> + sqlx_core::decode::Decode<'scalar, Driver>,
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
                                version_class: row
                                    .version_class
                                    .ok_or_else(|| corrupt("missing version_class"))?,
                                checksum_class: row
                                    .checksum_class
                                    .ok_or_else(|| corrupt("missing checksum_class"))?,
                                installed_class: row
                                    .installed_class
                                    .ok_or_else(|| corrupt("missing installed_class"))?,
                                execution_class: row
                                    .execution_class
                                    .ok_or_else(|| corrupt("missing execution_class"))?,
                                description: row
                                    .description
                                    .ok_or_else(|| corrupt("missing description"))?,
                                installed_on: row
                                    .installed_on
                                    .ok_or_else(|| corrupt("missing installed_on"))?,
                                success: row.success.ok_or_else(|| corrupt("missing success"))?,
                                execution_time: row
                                    .execution_time
                                    .ok_or_else(|| corrupt("missing execution_time"))?,
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
                    let migration = fleet_project_copy::RECEIVER_MIGRATOR
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
                                vec![0; 48],
                                schema.corpus().into(),
                            )
                            .await
                        }
                        HistoryCorruption::InvalidVersion => {
                            queries::bootstrap_probe_version(scope, 0, schema.corpus().into()).await
                        }
                        HistoryCorruption::UnknownVersion => {
                            queries::bootstrap_probe_version(scope, 999, schema.corpus().into())
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
                            fleet_project_lease::report_schema_support(scope, &report, self.now)
                                .await
                                .map_err(StoreError::backend)?;
                            let stored = fleet_project_lease::accepted_schema_support(scope, peer)
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
                            let _answer = fleet_project_copy::create_copy(scope, &request)
                                .await
                                .map_err(fleet_project_copy::receiver_rollback)?;
                            BootstrapProbeReply::Copy(Box::new(
                                fleet_project_copy::copy_status(scope)
                                    .await
                                    .map_err(fleet_project_copy::receiver_rollback)?,
                            ))
                        }
                        BootstrapProbe::Delivery => {
                            let allowance = fleet_mail_delivery::minimum_settlement_allowance::<
                                BoardDeliveryTypes,
                            >()
                            .map_err(StoreError::backend)?;
                            let limits =
                                fleet_mail_delivery::DeliveryLimits::new::<BoardDeliveryTypes>(
                                    1_000_000, 100, 1_000_000, allowance,
                                )
                                .map_err(StoreError::backend)?;
                            let item =
                                ItemId::try_from("019f0000-0000-7000-8000-000000000604".to_owned())
                                    .map_err(StoreError::backend)?;
                            let status =
                                fleet_mail_delivery::Outbox::<_, BoardDeliveryTypes, _>::new(
                                    scope,
                                    limits,
                                    &crate::fleet_sender_binding::FleetSenderBinding,
                                )
                                .relay_query(fleet_protocol::Submitter::Machine(peer), item)
                                .await
                                .map_err(StoreError::backend)?;
                            let result = QueryResult {
                                statuses: vec![status],
                            };
                            BootstrapProbeReply::Delivery(result)
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
