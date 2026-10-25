//! Retire only the acknowledged settled prefix, retaining progress across partial completion.
use super::*;
use message_board::{CancelExpansionSeq, HandoffSeq};
use message_board_storage::MailHandoffItem;

impl PublisherInner {
    pub(super) async fn retire_project_handoffs(
        &self,
        project: &ProjectId,
    ) -> Result<(), BoardError> {
        let mail = self
            .mail
            .upgrade()
            .ok_or_else(BoardError::board_unavailable)?;
        let store = self
            .admission
            .registry
            .existing_project(project)
            .await
            .map_err(|_| BoardError::board_unavailable())?
            .ok_or_else(BoardError::board_unavailable)?;
        let epoch = publication_epoch(&store).await?;
        let mut read = store
            .snapshot()
            .await
            .map_err(|_| BoardError::board_unavailable())?;
        let frontier = read
            .mail_settled_through()
            .await
            .map_err(|_| BoardError::board_unavailable())?;

        read.finish()
            .await
            .map_err(|_| BoardError::board_unavailable())?;
        let marks = mail.acknowledged_mail_changes(project).await?;
        let acknowledged = HandoffSeq::try_from(marks.settled_through.unwrap_or(0))
            .map_err(|_| BoardError::board_unavailable())?;
        let saved = self
            .progress
            .mail_retired_through(project)
            .await
            .map_err(|_| BoardError::board_unavailable())?;
        let baseline = store
            .verified_seed_frontier()
            .await
            .map_err(|_| BoardError::board_unavailable())?
            .unwrap_or(HandoffSeq::ZERO);
        // Seeded custody belongs to the predecessor. Only retire work above
        // that exact baseline, retaining any later local retirement progress.
        let mut after = saved.max(baseline);
        if acknowledged > frontier || after > frontier || baseline > acknowledged {
            return Err(BoardError::board_unavailable());
        }
        while after < acknowledged {
            if self.shutdown.is_cancelled() {
                break;
            }
            let mut read = store
                .snapshot()
                .await
                .map_err(|_| BoardError::board_unavailable())?;
            let page = read
                .mail_handoff_page(after, 100)
                .await
                .map_err(|_| BoardError::board_unavailable())?;
            read.finish()
                .await
                .map_err(|_| BoardError::board_unavailable())?;
            if page.is_empty() {
                return Err(BoardError::board_unavailable());
            }
            for handoff in page {
                if handoff.sequence > acknowledged {
                    return Ok(());
                }
                let retired = match &handoff.item {
                    MailHandoffItem::Delivery(envelope) => mail
                        .retire_frozen_original(envelope)
                        .await
                        .map_err(|_| BoardError::board_unavailable())?,
                    MailHandoffItem::CancelGroup(group) => {
                        let mut child_after = CancelExpansionSeq::ZERO;
                        loop {
                            let mut read = store
                                .snapshot()
                                .await
                                .map_err(|_| BoardError::board_unavailable())?;
                            let children = read
                                .mail_cancel_expansion_page(child_after, 100)
                                .await
                                .map_err(|_| BoardError::board_unavailable())?;
                            read.finish()
                                .await
                                .map_err(|_| BoardError::board_unavailable())?;
                            if children.is_empty() {
                                break;
                            }
                            for child in children {
                                child_after = child.sequence;
                                if child.expansion.group_handoff == group.id
                                    && !mail
                                        .retire_frozen_control(&child.expansion.envelope, epoch)
                                        .await
                                        .map_err(|_| BoardError::board_unavailable())?
                                {
                                    return Ok(());
                                }
                            }
                        }
                        true
                    }
                };
                if !retired {
                    return Ok(());
                }
                self.progress
                    .advance_mail_retired_through(project, handoff.sequence)
                    .await
                    .map_err(|_| BoardError::board_unavailable())?;
                after = handoff.sequence;
            }
        }
        Ok(())
    }
}
