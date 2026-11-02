//! Read the alarm's retention state and empty-work probe in one Fleet snapshot.
use crate::{
    fleet_feed_store,
    fleet_record_codec::corrupt,
    generated_fleet_feed as queries,
    step_driver::{StepCallback, StepResult},
};
use durable_steps::{AtomicStore, BoxFuture, StoreError, storage::TransactionScope};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::convert::Infallible;

#[derive(Clone, Copy)]
pub(super) enum AutomationMaintenance {
    Empty,
    Required,
}

pub(super) struct PrepareFleetAlarm {
    pub now: chrono::DateTime<chrono::Utc>,
}

impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for PrepareFleetAlarm
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = AutomationMaintenance;
    type Effect = Infallible;

    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Infallible>> {
        Box::pin(async move {
            let observed = queries::read_fleet_alarm_state(scope)
                .await
                .map_err(StoreError::backend)?;
            let first = observed
                .first()
                .ok_or_else(|| corrupt("alarm.state", "missing alarm snapshot"))?;
            let feed_exists =
                decode_presence_flag(first.feed_exists.as_deref(), "alarm.feed_exists")?;
            let maintenance = if decode_presence_flag(
                first.automation_work_exists.as_deref(),
                "alarm.automation_work_exists",
            )? {
                AutomationMaintenance::Required
            } else {
                AutomationMaintenance::Empty
            };
            let mut heads = Vec::new();
            for row in observed {
                if decode_presence_flag(row.state_exists.as_deref(), "alarm.state_exists")? {
                    heads.push(queries::StoredFleetFeedState {
                        state_id: row.state_id,
                        name: row.name,
                        last_position: row.last_position,
                        position_class: row.position_class,
                        retained_through: row.retained_through,
                        floor_class: row.floor_class,
                    });
                }
            }
            let state = fleet_feed_store::decode_state(heads, feed_exists)?;
            fleet_feed_store::prune_from_state(scope, self.now, state).await?;
            Ok((maintenance, Vec::new()))
        })
    }
}

fn decode_presence_flag(value: Option<&str>, field: &'static str) -> Result<bool, StoreError> {
    match value {
        Some("0") => Ok(false),
        Some("1") => Ok(true),
        _ => Err(corrupt(field, "invalid computed boolean")),
    }
}
