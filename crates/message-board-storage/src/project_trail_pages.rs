//! Trail keys are selected inside the requested feed interval before any body decode.
use crate::{FeedPageBounds, FeedPageDirection, FeedPageKey, ProjectSnapshot, ProjectStorageError};
use message_board::{TrailSubject, TrailTag};
#[derive(Clone, Copy)]
pub struct TrailPageFilter<'request> {
    pub subject: Option<&'request TrailSubject>,
    pub tags: &'request [TrailTag],
    pub include_redacted: bool,
    pub decisions_only: bool,
}
impl ProjectSnapshot {
    pub async fn trail_page_keys(
        &mut self,
        bounds: &FeedPageBounds,
        filter: TrailPageFilter<'_>,
    ) -> Result<Vec<FeedPageKey>, ProjectStorageError> {
        let subject = filter.subject.map(serde_json::to_string).transpose()?;
        let tags = serde_json::to_string(filter.tags)?;
        let include = filter.include_redacted;
        let decisions = filter.decisions_only;
        let start =
            i64::try_from(bounds.after.get()).map_err(|_| ProjectStorageError::InvalidFeed)?;
        let end =
            i64::try_from(bounds.through.get()).map_err(|_| ProjectStorageError::InvalidFeed)?;
        let limit = i64::try_from(bounds.limit).map_err(|_| ProjectStorageError::InvalidFeed)?;
        macro_rules! decode_rows {
            ($rows:expr) => {
                $rows
                    .into_iter()
                    .map(|row| {
                        self.feed_page_key(
                            row.position,
                            row.epoch,
                            "workTrailRow".to_owned(),
                            row.id,
                            row.router_time,
                            row.time_basis,
                        )
                    })
                    .collect()
            };
        }
        match bounds.direction {
            FeedPageDirection::Forward => decode_rows!(
                sqlx_turso::query_file!(
                    "src/project_trail_page.sql",
                    subject,
                    tags,
                    include,
                    decisions,
                    start,
                    end,
                    limit
                )
                .fetch_all(&mut self.transaction)
                .await?
            ),
            FeedPageDirection::Backward => decode_rows!(
                sqlx_turso::query_file!(
                    "src/project_trail_page_backward.sql",
                    subject,
                    tags,
                    include,
                    decisions,
                    start,
                    end,
                    limit
                )
                .fetch_all(&mut self.transaction)
                .await?
            ),
        }
    }
}
