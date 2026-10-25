//! A reader records only the applied cursor it supplied, never the page offered back.
use crate::{
    fleet_record_codec::{check_private_id, corrupt, required},
    generated_fleet_feed as queries,
};
use durable_steps::{StoreError, Uuid, storage::TransactionScope};
use message_board::{FleetPosition, ServiceId};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub(crate) struct FeedReader {
    pub installation: ServiceId,
    pub applied: FleetPosition,
}

pub(crate) async fn record<Scope: TransactionScope>(
    scope: &mut Scope,
    installation: &ServiceId,
    applied: FleetPosition,
    row_id: Uuid,
) -> Result<(), StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
{
    crate::fleet_row_ids::validate_row_id(row_id)?;
    i64::try_from(applied.get()).map_err(StoreError::backend)?;
    queries::store_feed_reader(
        scope,
        row_id.to_string(),
        installation.as_str().into(),
        applied.get().to_string(),
    )
    .await
    .map_err(StoreError::backend)
}

pub(crate) async fn readers<Scope: TransactionScope>(
    scope: &mut Scope,
    head: FleetPosition,
) -> Result<Vec<FeedReader>, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    queries::read_feed_readers(scope)
        .await
        .map_err(StoreError::backend)?
        .into_iter()
        .map(|row| {
            check_private_id(
                &required(row.reader_id, "feed_readers.reader_id")?,
                "feed_readers.reader_id",
            )?;
            let installation =
                ServiceId::try_from(required(row.installation, "feed_readers.installation")?)
                    .map_err(|error| corrupt("feed_readers.installation", error))?;
            let encoded = required(row.last_read_position, "feed_readers.last_read_position")?;
            let value = encoded
                .parse::<u64>()
                .map_err(|error| corrupt("feed_readers.last_read_position", error))?;
            if value.to_string() != encoded
                || required(row.position_class, "feed_readers.position_class")? != "integer"
                || i64::try_from(value).is_err()
            {
                return Err(corrupt(
                    "feed_readers.last_read_position",
                    "not a canonical nonnegative SQLite integer",
                ));
            }
            let applied = FleetPosition::try_from(value).map_err(StoreError::backend)?;
            if applied > head {
                return Err(corrupt(
                    "feed_readers.last_read_position",
                    "reader is beyond the committed feed",
                ));
            }
            Ok(FeedReader {
                installation,
                applied,
            })
        })
        .collect()
}
