//! Fleet keysets keep selection, body validation and response budgeting in that order.
use super::{BoardOperations, admission_error};
use message_board::{BoardError, Page, PageRequest, Record};
use message_board_storage::FleetListFilter;

impl BoardOperations<'_> {
    pub(super) async fn fleet_list_page(
        &self,
        selection: FleetListFilter<'_>,
        page: PageRequest,
        operation: &str,
        filter: String,
    ) -> Result<Page<Record>, BoardError> {
        let after = self.metadata_after(&page, operation, &filter).await?;
        let limit =
            usize::try_from(page.limit.get()).map_err(|_| BoardError::board_unavailable())?;
        let mut fleet = self
            .admission()?
            .fleet
            .read_snapshot()
            .await
            .map_err(|error| admission_error(error.into()))?;
        let mut keys = fleet
            .list_page_keys(selection, &after, u64::from(page.limit.get()) + 1)
            .await
            .map_err(|error| {
                admission_error(collaboration_runtime::FleetClientError::from(error).into())
            })?;
        let mut has_more = keys.len() > limit;
        keys.truncate(limit);
        let stored = fleet.record_batch(&keys).await.map_err(|error| {
            admission_error(collaboration_runtime::FleetClientError::from(error).into())
        })?;
        fleet.finish().await.map_err(|error| {
            admission_error(collaboration_runtime::FleetClientError::from(error).into())
        })?;
        if stored.len() != keys.len() {
            return Err(BoardError::board_unavailable());
        }
        let mut records = Vec::new();
        let mut bytes = 2_usize;
        for stored in stored {
            let record = stored.decode().map_err(|error| {
                admission_error(collaboration_runtime::FleetClientError::from(error).into())
            })?;
            let size = serde_json::to_vec(&record)
                .map_err(|_| BoardError::board_unavailable())?
                .len();
            if bytes.saturating_add(size).saturating_add(1) > 900 * 1024 {
                if records.is_empty() {
                    return Err(BoardError::board_unavailable());
                }
                has_more = true;
                break;
            }
            bytes = bytes.saturating_add(size).saturating_add(1);
            records.push(record);
        }
        self.selected_metadata_page(records, page, operation, filter, has_more, |row| {
            row.reference().id.as_str().to_owned()
        })
        .await
    }
}
