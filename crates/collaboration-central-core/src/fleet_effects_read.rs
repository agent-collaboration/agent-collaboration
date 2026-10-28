//! Fleet receipts are read by request and raw ordinal, independently of feed retention.

use crate::{
    fleet_decision_step, generated_statements,
    step_driver::{StepCallback, StepResult},
};
use durable_steps::{AtomicStore, BoxFuture, StoreError, storage::TransactionScope};
use message_board::{
    DecisionEffect, DecisionEffectsPage, RequestDecision, RequestEffectsCursor, RequestEffectsRead,
    RequestEffectsRefusal, RequestEffectsRequest, RequestEffectsScope, StoredDecision,
};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::convert::Infallible;

pub(crate) struct ReadFleetEffects(pub RequestEffectsRequest);
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for ReadFleetEffects
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = RequestEffectsRead;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move { Ok((read(scope, self.0).await?, Vec::new())) })
    }
}

async fn read<Scope: TransactionScope>(
    scope: &mut Scope,
    request: RequestEffectsRequest,
) -> Result<RequestEffectsRead, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let RequestEffectsScope::Fleet { workspace_id } = request.scope else {
        return Ok(refused(RequestEffectsRefusal::RequestNotInScope));
    };
    let Some(stored) =
        generated_statements::read_fleet_decision(scope, request.request.as_str().into())
            .await
            .map_err(StoreError::backend)?
    else {
        return Ok(refused(RequestEffectsRefusal::UnknownRequest));
    };
    let entry = fleet_decision_step::decode(stored)?;
    if entry.envelope.scope.workspace != workspace_id {
        return Ok(refused(RequestEffectsRefusal::RequestNotInScope));
    }
    let (header, change) = match entry.decision {
        StoredDecision::Rejected { reasons } => {
            return Ok(if request.after.is_none() {
                RequestEffectsRead::Decision {
                    decision: RequestDecision::Rejected { reasons },
                }
            } else {
                refused(RequestEffectsRefusal::CursorInvalid)
            });
        }
        StoredDecision::Accepted { effects, change } => (effects, change),
    };
    let (after, start) = match request.after {
        None => ("-1".to_owned(), 0),
        Some(RequestEffectsCursor::Fleet { ordinal })
            if ordinal < header.count && i64::try_from(ordinal).is_ok() =>
        {
            (ordinal.to_string(), ordinal.saturating_add(1))
        }
        Some(_) => return Ok(refused(RequestEffectsRefusal::CursorInvalid)),
    };
    let rows = generated_statements::read_fleet_receipt_page(
        scope,
        request.request.as_str().into(),
        after,
    )
    .await
    .map_err(StoreError::backend)?;
    let end = start.saturating_add(256).min(header.count);
    if u64::try_from(rows.len()).map_err(StoreError::backend)? != end.saturating_sub(start) {
        return Err(corrupt("receipt count disagrees with the immutable header"));
    }
    let entries = rows
        .into_iter()
        .enumerate()
        .map(|(offset, row)| {
            let ordinal = start
                .checked_add(u64::try_from(offset).map_err(StoreError::backend)?)
                .ok_or_else(|| corrupt("receipt ordinal overflow"))?;
            let id = required(row.receipt_id)?;
            let parsed = durable_steps::Uuid::parse_str(&id)
                .map_err(|error| corrupt(format!("invalid receipt identity: {error}")))?;
            if parsed.get_version_num() != 7 || parsed.to_string() != id {
                return Err(corrupt("noncanonical receipt identity"));
            }
            if required(row.ordinal_class)? != "integer" {
                return Err(corrupt("receipt ordinal is not stored as an integer"));
            }
            let stored_ordinal = required(row.ordinal)?;
            if stored_ordinal != ordinal.to_string() {
                return Err(corrupt(format!(
                    "expected ordinal {ordinal}, found {stored_ordinal}"
                )));
            }
            let encoded = required(row.effect)?;
            let effect: DecisionEffect =
                serde_json::from_str(&encoded).map_err(|error| corrupt(error.to_string()))?;
            message_board::validate_fleet_decision_effect(&effect)
                .map_err(|error| corrupt(error.to_string()))?;
            if serde_json::to_string(&effect).map_err(StoreError::backend)? != encoded {
                return Err(corrupt("noncanonical Fleet effect"));
            }
            Ok(effect)
        })
        .collect::<Result<Vec<_>, StoreError>>()?;
    let next = (end < header.count).then(|| RequestEffectsCursor::Fleet {
        ordinal: end.saturating_sub(1),
    });
    let effects = DecisionEffectsPage::from_page(entries, &header, next)
        .map_err(|error| corrupt(error.to_string()))?;
    Ok(RequestEffectsRead::Decision {
        decision: RequestDecision::Accepted { effects, change },
    })
}
fn refused(reason: RequestEffectsRefusal) -> RequestEffectsRead {
    RequestEffectsRead::Refused { reason }
}
fn required(value: Option<String>) -> Result<String, StoreError> {
    value.ok_or_else(|| corrupt("required receipt column is NULL"))
}
fn corrupt(message: impl Into<String>) -> StoreError {
    StoreError::Corrupt {
        field: "fleet_decision_receipts".into(),
        message: message.into(),
    }
}
