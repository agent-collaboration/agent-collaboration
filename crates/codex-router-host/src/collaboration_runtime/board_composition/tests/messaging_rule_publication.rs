//! Independent committed Inbox rows expose duplicate or lost publication.
use super::super::PrefixCase;
use super::*;
use board_sync_protocol::{BoardDeliveryBody, BoardDeliveryTypes};
use fleet_protocol::{DeliveryEnvelope, DeliveryItem, DeliveryStream, ItemId};

impl RuleFixture {
    pub async fn assert_successor_publication(
        &mut self,
        case: PrefixCase,
        original: &DeliveryItem<BoardDeliveryTypes>,
    ) -> TestResult {
        let publisher = self.successor.notifications.publisher.clone();
        publisher.queue_project(self.project.clone()).await?;
        timeout(BOUND, publisher.wait_project_idle_for_test(&self.project)).await?;
        let store = self
            .successor
            .registry
            .existing_project(&self.project)
            .await?
            .ok_or("successor store")?;
        let mut read = store.snapshot().await?;
        if matches!(case, PrefixCase::Neither) {
            assert!(
                read.mail_handoff_page(HandoffSeq::ZERO, 10)
                    .await?
                    .is_empty()
            );
            read.finish().await?;
            assert!(
                self.successor
                    .notifications
                    .rows()
                    .await?
                    .iter()
                    .all(|row| row.category != "outbox")
            );
            assert!(
                self.recipient
                    .pushed_envelopes(DeliveryStream::Items)
                    .await?
                    .is_empty()
            );
            return Ok(());
        }
        read.finish().await?;
        let pushed = self.wait_pushed_original(original.id()).await?;
        let [DeliveryEnvelope::Item(item)] = pushed.as_slice() else {
            return Err("takeover committed more than one item or a control".into());
        };
        let BoardDeliveryBody::Notification(body) = item.body() else {
            return Err("notification body missing".into());
        };
        assert_eq!(body.excerpt.as_str(), "rule cause survives takeover");
        match case {
            PrefixCase::Both => {
                assert_eq!(item.id(), original.id());
                assert_eq!(
                    serde_json::to_value(item)?,
                    serde_json::to_value(original)?,
                    "recipient envelope changed from the old holder's frozen input"
                );
            }
            PrefixCase::CauseOnly => assert_ne!(item.id(), original.id()),
            PrefixCase::Neither => return Err("lost tail reached positive publication".into()),
        }
        publisher.queue_project(self.project.clone()).await?;
        timeout(BOUND, publisher.wait_project_idle_for_test(&self.project)).await?;
        let replay = self
            .recipient
            .pushed_envelopes(DeliveryStream::Items)
            .await?;
        assert_eq!(serde_json::to_value(replay)?, serde_json::to_value(pushed)?);
        let rows = self.successor.notifications.rows().await?;
        assert_eq!(
            rows.iter().filter(|row| row.category == "outbox").count(),
            1
        );
        let mut read = store.snapshot().await?;
        assert_eq!(read.mail_handoff_page(HandoffSeq::ZERO, 10).await?.len(), 1);
        if matches!(case, PrefixCase::CauseOnly) {
            assert!(
                read.mail_handoff(&original.id().as_uuid().to_string().try_into()?)
                    .await?
                    .is_none()
            );
        }
        read.finish().await?;
        Ok(())
    }

    async fn wait_pushed_original(
        &self,
        original: ItemId,
    ) -> Result<Vec<DeliveryEnvelope<BoardDeliveryTypes>>, Box<dyn std::error::Error + Send + Sync>>
    {
        let pushed = timeout(BOUND, async {
            loop {
                let pushed = self
                    .recipient
                    .pushed_envelopes(DeliveryStream::Items)
                    .await?;
                if !pushed.is_empty() {
                    return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(pushed);
                }
                tokio::task::yield_now().await;
            }
        })
        .await;
        match pushed {
            Ok(result) => result,
            Err(error) => {
                let marks = self
                    .successor
                    .projects
                    .acknowledged_mail_changes(&self.project)
                    .await?;
                let rows = self
                    .successor
                    .notifications
                    .rows()
                    .await?
                    .into_iter()
                    .map(|row| (row.category, row.identity, row.content))
                    .collect::<Vec<_>>();
                let epoch = self
                    .successor
                    .notifications
                    .owner
                    .mail_submission_epoch(&original.as_uuid().to_string().try_into()?)
                    .await?;
                Err(format!("successor Push timed out: {error}; marks={marks:?}; epoch={epoch:?}; native={rows:?}").into())
            }
        }
    }
}
