//! Snapshot, previous holder, journal admission and handled mark share one transaction.
use crate::{
    fleet_record_codec::{corrupt, decode_json, required},
    generated_registry_lease as queries,
    machine_notice_outbox::{MachineNoticeInput, RegistryNoticeTask},
    registry_peer_lookup::service_for_peer,
    step_driver::{StepCallback, StepResult},
};
use board_sync_protocol::BoardNotice;
use durable_steps::{
    AtomicStore, BoxFuture, JournalScope, RequestKey, StoreError, storage::TransactionScope,
};
use fleet_sync_protocol::{DatabaseId, Epoch, SyncTime};
use message_board::{ProjectId, ServiceId};
use serde::{Deserialize, Serialize};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::convert::Infallible;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RegistryNoticeMark {
    database: DatabaseId,
    epoch: Epoch,
    holder: Option<ServiceId>,
}

pub(crate) enum RegistryNoticeStep {
    Pending { database: DatabaseId },
    Admit { database: DatabaseId, now: SyncTime },
}
pub(crate) enum RegistryNoticeWork {
    Current,
    Pending,
    Admitted,
    Unavailable,
}
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for RegistryNoticeStep
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = RegistryNoticeWork;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<RegistryNoticeWork, Infallible>> {
        Box::pin(async move {
            let database = match self {
                Self::Pending { database } | Self::Admit { database, .. } => database,
            };
            let Some(snapshot) = fleet_sync_lease::registry_snapshot(scope, database)
                .await
                .map_err(StoreError::backend)?
            else {
                return Ok((RegistryNoticeWork::Unavailable, Vec::new()));
            };
            if snapshot.database != database
                || snapshot
                    .grant
                    .as_ref()
                    .is_some_and(|grant| grant.epoch != snapshot.revision.epoch)
            {
                return Err(corrupt(
                    "registry_notice.snapshot",
                    "grant or database identity differs",
                ));
            }
            let holder = match snapshot.grant {
                Some(grant) => match service_for_peer(scope, grant.holder).await? {
                    Some(installation) => Some(installation),
                    None => return Ok((RegistryNoticeWork::Unavailable, Vec::new())),
                },
                None => None,
            };
            let current = RegistryNoticeMark {
                database,
                epoch: snapshot.revision.epoch,
                holder,
            };
            let previous = read_mark(scope, database).await?;
            if previous.as_ref() == Some(&current) {
                return Ok((RegistryNoticeWork::Current, Vec::new()));
            }
            if previous
                .as_ref()
                .is_some_and(|mark| mark.epoch > current.epoch)
            {
                return Err(corrupt(
                    "registry_notice.mark",
                    "handled epoch exceeds current snapshot",
                ));
            }
            if let Self::Admit { now, .. } = self {
                let has_notices = previous.as_ref().is_some_and(|mark| mark.holder.is_some())
                    || current.holder.is_some();
                let project =
                    ProjectId::try_from(database.to_string()).map_err(StoreError::backend)?;
                if let Some(RegistryNoticeMark {
                    epoch,
                    holder: Some(installation),
                    ..
                }) = previous
                {
                    admit::<Store>(
                        scope,
                        database,
                        epoch,
                        "superseded",
                        installation,
                        BoardNotice::LeaseSuperseded {
                            project: project.clone(),
                            epoch,
                        },
                        now,
                    )
                    .await?;
                }
                if let Some(installation) = &current.holder {
                    admit::<Store>(
                        scope,
                        database,
                        current.epoch,
                        "granted",
                        installation.clone(),
                        BoardNotice::LeaseGranted {
                            project,
                            epoch: current.epoch,
                        },
                        now,
                    )
                    .await?;
                }
                queries::store_registry_notice_mark(
                    scope,
                    database.to_string(),
                    serde_json::to_string(&current).map_err(StoreError::backend)?,
                )
                .await
                .map_err(StoreError::backend)?;
                return Ok((
                    if has_notices {
                        RegistryNoticeWork::Admitted
                    } else {
                        RegistryNoticeWork::Current
                    },
                    Vec::new(),
                ));
            }
            Ok((RegistryNoticeWork::Pending, Vec::new()))
        })
    }
}

async fn read_mark<Scope: TransactionScope>(
    scope: &mut Scope,
    database: DatabaseId,
) -> Result<Option<RegistryNoticeMark>, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    queries::read_registry_notice_mark(scope, database.to_string())
        .await
        .map_err(StoreError::backend)?
        .map(|row| {
            if required(row.database_id, "registry_notice.database_id")? != database.to_string() {
                return Err(corrupt("registry_notice.database_id", "identity differs"));
            }
            let mark: RegistryNoticeMark = decode_json(
                &required(row.mark, "registry_notice.mark")?,
                "registry_notice.mark",
            )?;
            if mark.database != database {
                return Err(corrupt("registry_notice.mark", "database identity differs"));
            }
            Ok(mark)
        })
        .transpose()
}

async fn admit<Store: AtomicStore>(
    scope: &mut Store::Scope<'_>,
    database: DatabaseId,
    epoch: Epoch,
    kind: &str,
    installation: ServiceId,
    notice: BoardNotice,
    now: SyncTime,
) -> Result<(), StoreError> {
    let key = RequestKey::new(format!(
        "registry-notice:{database}:{}:{kind}:{}",
        epoch.value(),
        installation.as_str()
    ))
    .map_err(StoreError::backend)?;
    scope
        .admit::<RegistryNoticeTask<Store>>(
            &key,
            &MachineNoticeInput {
                installation,
                notice,
            },
        )
        .await
        .map_err(StoreError::backend)?;
    scope.ensure_wake(now).await
}
