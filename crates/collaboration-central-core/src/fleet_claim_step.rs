//! A claim's attempt stays settled even when an earlier reservation arrives late.
use crate::{
    ClaimReservationOutcome, ClaimReservationState,
    fleet_record_codec::{corrupt, decode_json, required},
    fleet_record_store, generated_fleet_lease as queries,
    step_driver::{StepCallback, StepResult},
};
use durable_steps::{AtomicStore, BoxFuture, StoreError, storage::TransactionScope};
use fleet_sync_protocol::{DatabaseId, GateAnswer, GateReason, GateReasonCode, TransferId};
use message_board::{BoardState, ProjectId, Record};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::convert::Infallible;

pub(crate) enum ClaimStep {
    Reserve {
        database: DatabaseId,
        attempt: TransferId,
    },
    Refuse {
        database: DatabaseId,
        attempt: TransferId,
        now: chrono::DateTime<chrono::Utc>,
        row_ids: crate::fleet_row_ids::FleetRowIds,
    },
}

pub(crate) enum ClaimStepResult {
    Reservation(GateAnswer),
    Settled,
}

impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for ClaimStep
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = ClaimStepResult;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Infallible>> {
        Box::pin(async move {
            match self {
                Self::Reserve { database, attempt } => Ok((
                    ClaimStepResult::Reservation(reserve(scope, database, attempt).await?),
                    Vec::new(),
                )),
                Self::Refuse {
                    database,
                    attempt,
                    now,
                    mut row_ids,
                } => {
                    let previous_feed = crate::fleet_feed_store::high_water(scope).await?;
                    match read(scope, database, attempt).await? {
                        Some(ClaimReservationState::Settled {
                            outcome: ClaimReservationOutcome::Granted,
                        }) => {
                            return Err(corrupt(
                                "claim.state",
                                "cannot refuse a granted settlement",
                            ));
                        }
                        Some(ClaimReservationState::Settled {
                            outcome: ClaimReservationOutcome::Refused,
                        }) => {}
                        None | Some(ClaimReservationState::Active) => {
                            write(
                                scope,
                                database,
                                attempt,
                                ClaimReservationState::Settled {
                                    outcome: ClaimReservationOutcome::Refused,
                                },
                            )
                            .await?;
                        }
                    }
                    let project = ProjectId::try_from(database.as_uuid().to_string())
                        .map_err(|error| corrupt("claim.project_id", error))?;
                    crate::fleet_archive_completion::complete_project(
                        scope,
                        &project,
                        now,
                        &mut row_ids,
                    )
                    .await?;
                    crate::fleet_notice_admission::admit_mutation_notices::<Store, Driver>(
                        scope,
                        previous_feed,
                        runtime_primitives::SyncTime::from_unix_millis(now.timestamp_millis()),
                    )
                    .await?;
                    Ok((ClaimStepResult::Settled, Vec::new()))
                }
            }
        })
    }
}

async fn reserve<Scope: TransactionScope>(
    scope: &mut Scope,
    database: DatabaseId,
    attempt: TransferId,
) -> Result<GateAnswer, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String:
        Type<Scope::Database> + Encode<'value, Scope::Database> + Decode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    if let Some(state) = read(scope, database, attempt).await? {
        return match state {
            ClaimReservationState::Active => Ok(GateAnswer::Reserved),
            ClaimReservationState::Settled { outcome } => refusal(
                "claim_already_settled",
                format!(
                    "claim attempt already settled as {}",
                    match outcome {
                        ClaimReservationOutcome::Granted => "granted",
                        ClaimReservationOutcome::Refused => "refused",
                    }
                ),
            ),
        };
    }
    let project = ProjectId::try_from(database.as_uuid().to_string())
        .map_err(|error| corrupt("claim.project_id", error))?;
    let Some(stored) = fleet_record_store::read(scope, &project.into()).await? else {
        return refusal("not_found", "claim project is absent");
    };
    let Record::DirectoryEntry(entry) = stored.record else {
        return Err(corrupt("claim.project", "wrong record kind"));
    };
    let stored = fleet_record_store::read(scope, &entry.board.into())
        .await?
        .ok_or_else(|| corrupt("claim.board", "parent board is absent"))?;
    let Record::Board(board) = stored.record else {
        return Err(corrupt("claim.board", "wrong record kind"));
    };
    match board.state {
        BoardState::Archiving => return refusal("board_archiving", "claim board is archiving"),
        BoardState::Archived => return refusal("board_archived", "claim board is archived"),
        BoardState::Open | BoardState::Closed => {}
    }
    write(scope, database, attempt, ClaimReservationState::Active).await?;
    Ok(GateAnswer::Reserved)
}

pub(crate) async fn read<Scope: TransactionScope>(
    scope: &mut Scope,
    database: DatabaseId,
    attempt: TransferId,
) -> Result<Option<ClaimReservationState>, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    queries::read_claim_reservation(scope, attempt.as_uuid().to_string())
        .await
        .map_err(StoreError::backend)?
        .map(|stored| {
            if required(stored.project_id, "claim.project_id")? != database.as_uuid().to_string() {
                return Err(corrupt(
                    "claim.project_id",
                    "attempt belongs to another project",
                ));
            }
            decode_json(&required(stored.state, "claim.state")?, "claim.state")
        })
        .transpose()
}

pub(crate) async fn write<Scope: TransactionScope>(
    scope: &mut Scope,
    database: DatabaseId,
    attempt: TransferId,
    state: ClaimReservationState,
) -> Result<(), StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
{
    queries::store_claim_reservation(
        scope,
        attempt.as_uuid().to_string(),
        database.as_uuid().to_string(),
        serde_json::to_string(&state).map_err(StoreError::backend)?,
    )
    .await
    .map_err(StoreError::backend)?;
    Ok(())
}

fn refusal(code: &str, detail: impl Into<String>) -> Result<GateAnswer, StoreError> {
    Ok(GateAnswer::Refused {
        reason: GateReason {
            code: GateReasonCode::new(code).map_err(StoreError::backend)?,
            detail: detail.into(),
        },
    })
}
