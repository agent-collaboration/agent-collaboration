//! Covered content reads report their internal mark result separately from the data.
use super::{BoardOperations, admission_error};
use message_board::*;
impl BoardOperations<'_> {
    pub(super) async fn finish_project_read(
        &self,
        project: &ProjectId,
        scope: ReadScope,
        reader: Option<&Identity>,
        covered: Option<FeedCursor>,
    ) -> Result<Option<ReadMarkOutcome>, BoardError> {
        let Some(reader) = reader else {
            return Ok(None);
        };
        let Some(covered) = covered else {
            return Ok(Some(ReadMarkOutcome::Unchanged {}));
        };
        let outcome = self
            .admission()?
            .mark_covered_read(project, reader, scope, covered)
            .await
            .map_err(admission_error)?;
        if matches!(outcome, ReadMarkOutcome::Advanced { .. })
            && let Some(delivery) = &self.identity.subscription_delivery
            && let Err(error) = delivery
                .reconcile_project(reader.clone(), project.clone())
                .await
        {
            tracing::warn!(%error,"covered read delivery reconciliation unavailable");
        }
        Ok(Some(outcome))
    }
}
