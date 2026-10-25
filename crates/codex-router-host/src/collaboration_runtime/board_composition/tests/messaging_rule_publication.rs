//! Independent receiver reads and native rows expose duplicate or lost publication.
use super::super::{
    PrefixCase,
    relay_transport::{operation, relay_request},
};
use super::*;
use board_sync_protocol::{BoardDeliveryBody, BoardDeliveryTypes};
use fleet_sync_protocol::{DeliveryEnvelope, DeliveryItem, ItemId};

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
            assert_eq!(
                operation(
                    &mut self.recipient.socket,
                    relay_request(
                        "fetch",
                        json!({"stream":"items","after":0,"maxItems":10,"maxBytes":1_000_000})
                    )
                )
                .await?,
                json!({"kind":"delivery","value":{"kind":"fetch","value":{"stream":"items","entries":[],"through":0}}})
            );
            return Ok(());
        }
        read.finish().await?;
        let fetched = self.fetch_published_original(original.id()).await?;
        assert_eq!(fetched.pointer("/value/value/through"), Some(&json!(1)));
        let entries = fetched
            .pointer("/value/value/entries")
            .and_then(serde_json::Value::as_array)
            .ok_or("fetched items")?;
        let [entry] = entries.as_slice() else {
            return Err("takeover published more than one item".into());
        };
        assert_eq!(entry.pointer("/value/seq"), Some(&json!(1)));
        let envelope: DeliveryEnvelope<BoardDeliveryTypes> = serde_json::from_value(
            entry
                .pointer("/value/item")
                .ok_or("fetched envelope")?
                .clone(),
        )?;
        let DeliveryEnvelope::Item(item) = envelope else {
            return Err("notification became a control".into());
        };
        let BoardDeliveryBody::Notification(body) = item.body() else {
            return Err("notification body missing".into());
        };
        assert_eq!(body.excerpt.as_str(), "rule cause survives takeover");
        match case {
            PrefixCase::Both => {
                assert_eq!(item.id(), original.id());
                assert_eq!(
                    serde_json::to_value(&item)?,
                    serde_json::to_value(original)?,
                    "recipient envelope changed from the old holder's frozen input"
                );
            }
            PrefixCase::CauseOnly => assert_ne!(item.id(), original.id()),
            PrefixCase::Neither => return Err("lost tail reached positive publication".into()),
        }
        publisher.queue_project(self.project.clone()).await?;
        timeout(BOUND, publisher.wait_project_idle_for_test(&self.project)).await?;
        let replay = operation(
            &mut self.recipient.socket,
            relay_request(
                "fetch",
                json!({"stream":"items","after":0,"maxItems":10,"maxBytes":1_000_000}),
            ),
        )
        .await?;
        assert_eq!(replay, fetched);
        let rows = self.successor.notifications.rows().await?;
        assert_eq!(
            rows.iter().filter(|row| row.category == "outbox").count(),
            1
        );
        let mut read = store.snapshot().await?;
        let handoffs = read.mail_handoff_page(HandoffSeq::ZERO, 10).await?;
        assert_eq!(handoffs.len(), 1);
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
    async fn fetch_published_original(
        &mut self,
        original: ItemId,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        let fetched = timeout(BOUND, async {
            loop {
                let fetched = operation(
                    &mut self.recipient.socket,
                    relay_request(
                        "fetch",
                        json!({"stream":"items","after":0,"maxItems":10,"maxBytes":1_000_000}),
                    ),
                )
                .await?;
                if fetched
                    .pointer("/value/value/entries")
                    .and_then(serde_json::Value::as_array)
                    .is_some_and(|entries| !entries.is_empty())
                {
                    return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(fetched);
                }
                tokio::task::yield_now().await;
            }
        })
        .await;
        match fetched {
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
                Err(format!("successor mailbox publication timed out: {error}; marks={marks:?}; epoch={epoch:?}; native={rows:?}").into())
            }
        }
    }
}
