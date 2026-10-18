//! Scope-bound signed cursors retain the existing board pagination contract.
use super::{BoardOperations, admission_error};
use message_board::*;
use message_board_storage::{decode_board_cursor, encode_board_cursor};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MetadataCursor {
    operation: String,
    filter: String,
    last_key: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ProjectReadCursor {
    pub operation: String,
    pub filter: String,
    pub high_water: AggregateCursor,
    pub next: AggregateCursor,
}
pub(super) fn fingerprint<TRequest: Serialize>(request: &TRequest) -> Result<String, BoardError> {
    let mut value = serde_json::to_value(request).map_err(|_| BoardError::board_unavailable())?;
    if let Some(fields) = value.as_object_mut() {
        fields.remove("page");
    }
    serde_json::to_string(&value).map_err(|_| BoardError::board_unavailable())
}
pub(super) fn invalid_cursor() -> BoardError {
    BoardError {
        kind: BoardFailureKind::InvalidCursor,
        stage: BoardFailureStage::Validation,
        message: "Cursor is invalid for this operation and filter.".to_owned(),
        next_action: BoardNextAction::CorrectRequest,
        details: BoardErrorDetails::None,
    }
}
impl BoardOperations<'_> {
    /// SQL has already limited and merged candidates; an extra key proves continuation.
    pub(super) async fn selected_metadata_page<TRecord: Serialize>(
        &self,
        records: Vec<TRecord>,
        page: PageRequest,
        operation: &str,
        filter: String,
        has_more: bool,
        key: impl Fn(&TRecord) -> String,
    ) -> Result<Page<TRecord>, BoardError> {
        let mut result = self
            .metadata_page(records, page, operation, filter.clone(), &key)
            .await?;
        if has_more && result.next_cursor.is_none() {
            let last = result
                .records
                .last()
                .ok_or_else(BoardError::board_unavailable)?;
            let signing_key = self
                .admission()?
                .registry
                .cursor_key()
                .await
                .map_err(|error| admission_error(error.into()))?;
            result.next_cursor = Some(
                encode_board_cursor(
                    signing_key,
                    &MetadataCursor {
                        operation: operation.to_owned(),
                        filter,
                        last_key: key(last),
                    },
                )
                .map_err(|_| invalid_cursor())?,
            );
        }
        Ok(result)
    }
    pub(super) async fn metadata_after(
        &self,
        page: &PageRequest,
        operation: &str,
        filter: &str,
    ) -> Result<String, BoardError> {
        let Some(cursor) = &page.cursor else {
            return Ok(String::new());
        };
        let key = self
            .admission()?
            .registry
            .cursor_key()
            .await
            .map_err(|error| admission_error(error.into()))?;
        let cursor: MetadataCursor =
            decode_board_cursor(key, cursor).map_err(|_| invalid_cursor())?;
        if cursor.operation != operation || cursor.filter != filter {
            return Err(invalid_cursor());
        }
        Ok(cursor.last_key)
    }
    pub(super) async fn metadata_page<TRecord: Serialize>(
        &self,
        mut records: Vec<TRecord>,
        page: PageRequest,
        operation: &str,
        filter: String,
        key: impl Fn(&TRecord) -> String,
    ) -> Result<Page<TRecord>, BoardError> {
        let signing_key = self
            .admission()?
            .registry
            .cursor_key()
            .await
            .map_err(|error| admission_error(error.into()))?;
        let after = self.metadata_after(&page, operation, &filter).await?;
        records.sort_by_key(&key);
        let mut selected = Vec::new();
        let mut byte_count = 2_usize;
        let mut has_more = false;
        for record in records.into_iter().filter(|record| key(record) > after) {
            let bytes = serde_json::to_vec(&record)
                .map_err(|_| BoardError::board_unavailable())?
                .len();
            if selected.len() >= usize::try_from(page.limit.get()).map_err(|_| invalid_cursor())?
                || byte_count.saturating_add(bytes).saturating_add(1) > 900 * 1024
            {
                if selected.is_empty() {
                    return Err(BoardError::board_unavailable());
                }
                has_more = true;
                break;
            }
            byte_count = byte_count.saturating_add(bytes).saturating_add(1);
            selected.push(record);
        }
        let next_cursor = if has_more {
            let last = selected.last().ok_or_else(invalid_cursor)?;
            Some(
                encode_board_cursor(
                    signing_key,
                    &MetadataCursor {
                        operation: operation.to_owned(),
                        filter,
                        last_key: key(last),
                    },
                )
                .map_err(|_| invalid_cursor())?,
            )
        } else {
            None
        };
        Ok(Page {
            records: selected,
            next_cursor,
        })
    }
    pub(super) async fn decode_read_cursor(
        &self,
        page: &PageRequest,
        operation: &str,
        filter: &str,
    ) -> Result<Option<ProjectReadCursor>, BoardError> {
        let Some(cursor) = &page.cursor else {
            return Ok(None);
        };
        let key = self
            .admission()?
            .registry
            .cursor_key()
            .await
            .map_err(|error| admission_error(error.into()))?;
        let cursor: ProjectReadCursor =
            decode_board_cursor(key, cursor).map_err(|_| invalid_cursor())?;
        if cursor.operation != operation || cursor.filter != filter {
            return Err(invalid_cursor());
        }
        Ok(Some(cursor))
    }
    pub(super) async fn encode_read_cursor(
        &self,
        cursor: &ProjectReadCursor,
    ) -> Result<String, BoardError> {
        let key = self
            .admission()?
            .registry
            .cursor_key()
            .await
            .map_err(|error| admission_error(error.into()))?;
        encode_board_cursor(key, cursor).map_err(|_| invalid_cursor())
    }
}
