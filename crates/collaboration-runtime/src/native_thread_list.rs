//! Bounded native inventory pages over recorded update times and Session identities.
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use collaboration_contracts::HubSessionSummary;
use message_board::SessionId;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::cmp::Ordering;

pub(super) enum NativeThreadListError {
    InvalidParams,
    UnsupportedOrdering(&'static str),
    Unavailable,
}

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SortDirection {
    Asc,
    #[default]
    Desc,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum SortKey {
    UpdatedAt,
    CreatedAt,
    RecencyAt,
    SectionPosition,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadListRequest {
    limit: Option<u32>,
    cursor: Option<String>,
    sort_key: Option<SortKey>,
    sort_direction: Option<SortDirection>,
    archived: Option<bool>,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum ThreadListCursor {
    #[serde(rename_all = "camelCase")]
    After {
        updated_at_seconds: i64,
        session_id: SessionId,
    },
    #[serde(rename_all = "camelCase")]
    AtTimestamp { updated_at_seconds: i64 },
}

impl ThreadListCursor {
    fn encode(&self) -> Result<String, NativeThreadListError> {
        serde_json::to_vec(self)
            .map(|bytes| URL_SAFE_NO_PAD.encode(bytes))
            .map_err(|_| NativeThreadListError::Unavailable)
    }

    fn decode(cursor: &str) -> Result<Self, NativeThreadListError> {
        let bytes = URL_SAFE_NO_PAD
            .decode(cursor)
            .map_err(|_| NativeThreadListError::InvalidParams)?;
        let parsed: Self =
            serde_json::from_slice(&bytes).map_err(|_| NativeThreadListError::InvalidParams)?;
        let timestamp = match &parsed {
            Self::After {
                updated_at_seconds, ..
            }
            | Self::AtTimestamp { updated_at_seconds } => *updated_at_seconds,
        };
        if timestamp < 0 {
            return Err(NativeThreadListError::InvalidParams);
        }
        Ok(parsed)
    }
}

pub(super) struct NativeThreadListOptions {
    limit: usize,
    cursor: Option<ThreadListCursor>,
    direction: SortDirection,
    archived: bool,
}

pub(super) struct NativeThreadListPage {
    pub(super) summaries: Vec<HubSessionSummary>,
    pub(super) next_cursor: Option<String>,
    pub(super) backwards_cursor: Option<String>,
}

impl NativeThreadListOptions {
    pub(super) fn parse(params: &Value) -> Result<Self, NativeThreadListError> {
        let request: ThreadListRequest = serde_json::from_value(params.clone())
            .map_err(|_| NativeThreadListError::InvalidParams)?;
        // Preserve Router's update-ordered inventory default. Explicit native
        // creation/recency/section ordering cannot be inferred from update time.
        match request.sort_key {
            None | Some(SortKey::UpdatedAt) => {}
            Some(SortKey::CreatedAt) => {
                return Err(NativeThreadListError::UnsupportedOrdering(
                    "creation time is not recorded for provider sessions",
                ));
            }
            Some(SortKey::RecencyAt) => {
                return Err(NativeThreadListError::UnsupportedOrdering(
                    "recency time is not recorded for provider sessions",
                ));
            }
            Some(SortKey::SectionPosition) => {
                return Err(NativeThreadListError::UnsupportedOrdering(
                    "provider session sections are not recorded",
                ));
            }
        }
        Ok(Self {
            // Match pinned native thread_processor.rs:2782-2786, including zero.
            limit: usize::try_from(request.limit.unwrap_or(25).clamp(1, 100))
                .map_err(|_| NativeThreadListError::InvalidParams)?,
            cursor: request
                .cursor
                .as_deref()
                .map(ThreadListCursor::decode)
                .transpose()?,
            direction: request.sort_direction.unwrap_or_default(),
            archived: request.archived.unwrap_or(false),
        })
    }

    pub(super) fn archived(&self) -> bool {
        self.archived
    }

    pub(super) fn page(
        self,
        mut summaries: Vec<HubSessionSummary>,
    ) -> Result<NativeThreadListPage, NativeThreadListError> {
        summaries.sort_by(|left, right| {
            compare_timestamps(
                left.updated_at_seconds,
                right.updated_at_seconds,
                self.direction,
            )
            .then_with(|| left.session.session_id.cmp(&right.session.session_id))
        });
        if let Some(cursor) = self.cursor {
            summaries.retain(|summary| match &cursor {
                ThreadListCursor::After {
                    updated_at_seconds,
                    session_id,
                } => compare_timestamps(
                    summary.updated_at_seconds,
                    *updated_at_seconds,
                    self.direction,
                )
                .then_with(|| summary.session.session_id.cmp(session_id))
                .is_gt(),
                // Reversing starts at the page timestamp, so same-second rows
                // and updates at that timestamp are not silently skipped.
                ThreadListCursor::AtTimestamp { updated_at_seconds } => !compare_timestamps(
                    summary.updated_at_seconds,
                    *updated_at_seconds,
                    self.direction,
                )
                .is_lt(),
            });
        }
        let has_more = summaries.len() > self.limit;
        summaries.truncate(self.limit);
        let next_cursor = if has_more {
            summaries
                .last()
                .map(|summary| {
                    ThreadListCursor::After {
                        updated_at_seconds: summary.updated_at_seconds,
                        session_id: summary.session.session_id.clone(),
                    }
                    .encode()
                })
                .transpose()?
        } else {
            None
        };
        let backwards_cursor = summaries
            .first()
            .map(|summary| {
                ThreadListCursor::AtTimestamp {
                    updated_at_seconds: summary.updated_at_seconds,
                }
                .encode()
            })
            .transpose()?;
        Ok(NativeThreadListPage {
            summaries,
            next_cursor,
            backwards_cursor,
        })
    }
}

fn compare_timestamps(left: i64, right: i64, direction: SortDirection) -> Ordering {
    match direction {
        SortDirection::Asc => left.cmp(&right),
        SortDirection::Desc => right.cmp(&left),
    }
}
