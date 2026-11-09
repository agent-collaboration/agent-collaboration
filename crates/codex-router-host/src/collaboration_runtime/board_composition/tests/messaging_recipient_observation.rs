//! The recipient's actual Host pump commits Push into its SDK Inbox.
use super::*;
use board_sync_protocol::BoardDeliveryTypes;
use fleet_protocol::{
    CancelBody, DeliveryCancel, DeliveryEnvelope, DeliveryItemRef, DeliveryStream, ItemDigest,
    ItemId, OperationAnswer, OperationRequest, Recipient,
};

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommittedInboxHeader {
    id: ItemId,
    digest: ItemDigest,
    sender: collaboration_mail_protocol::MailSender,
    recipient: Recipient<message_board::SessionRef>,
    cancel_target: Option<ItemId>,
    valid_until: Option<runtime_primitives::SyncTime>,
    stream: DeliveryStream,
    submitter: fleet_protocol::Submitter,
}

impl RuleMachine {
    pub async fn assert_retained_original(
        &self,
        expected: &fleet_protocol::DeliveryItem<BoardDeliveryTypes>,
    ) -> TestResult {
        let rows = self.notifications.rows().await?;
        let originals = rows
            .iter()
            .filter(|row| row.category == "inbox")
            .map(|row| {
                serde_json::from_str::<CommittedInboxHeader>(
                    row.content.as_deref().ok_or("committed inbox header")?,
                )
                .map_err(Into::into)
            })
            .collect::<Result<Vec<_>, Box<dyn std::error::Error + Send + Sync>>>()?;
        let originals = originals
            .iter()
            .filter(|header| header.stream == DeliveryStream::Items)
            .collect::<Vec<_>>();
        assert_eq!(originals.len(), 1, "Push replay allocated another original");
        let header = originals[0];
        assert_eq!(header.id, expected.id());
        assert_eq!(
            &header.digest,
            expected.digest(),
            "retained digest no longer binds the observed frozen bytes"
        );
        assert_eq!(&header.sender, expected.sender());
        assert_eq!(&header.recipient, expected.recipient());
        assert_eq!(header.valid_until, expected.valid_until());
        assert_eq!(header.cancel_target, None);
        let collaboration_mail_protocol::MailSender::Project { project } = expected.sender() else {
            return Err("expected project sender".into());
        };
        assert_eq!(
            header.submitter,
            fleet_protocol::Submitter::Project(fleet_protocol::DatabaseId::try_from(
                project.as_str().to_owned()
            )?)
        );
        Ok(())
    }

    pub async fn operation(
        &self,
        request: serde_json::Value,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        let request: OperationRequest<board_sync_protocol::BoardConsumerProtocol> =
            serde_json::from_value(request)?;
        let wire = self.machine.observe_mail_wire(None)?;
        let answer: OperationAnswer<board_sync_protocol::BoardConsumerProtocol> = self
            .projects
            .requests
            .request(request)
            .await
            .map_err(|error| {
                format!(
                    "recipient operation failed: {error}; wire={:?}",
                    wire.events()
                )
            })?;
        Ok(serde_json::to_value(answer)?)
    }

    pub async fn pushed_envelopes(
        &self,
        stream: DeliveryStream,
    ) -> Result<Vec<DeliveryEnvelope<BoardDeliveryTypes>>, Box<dyn std::error::Error + Send + Sync>>
    {
        let mut envelopes = Vec::new();
        // The SDK's public test observation validates each stored header itself.
        // Item bodies are then resolved by the same SDK used for actual delivery.
        for row in self
            .notifications
            .rows()
            .await?
            .into_iter()
            .filter(|row| row.category == "inbox")
        {
            let header: CommittedInboxHeader =
                serde_json::from_str(row.content.as_deref().ok_or("committed inbox header")?)?;
            if header.stream != stream {
                continue;
            }
            let envelope = if let Some(target) = header.cancel_target {
                // A compact control's entire body is its target. Rebuild through
                // the validated constructor and compare its retained digest.
                let cancel = DeliveryCancel::new(
                    header.id,
                    header.sender,
                    header.recipient,
                    header.valid_until,
                    CancelBody { target },
                )?;
                assert_eq!(cancel.digest(), &header.digest);
                DeliveryEnvelope::Cancel(cancel)
            } else {
                DeliveryEnvelope::Item(
                    self.notifications
                        .mail_runtime()
                        .runtime_test_pushed_item(DeliveryItemRef {
                            item: header.id,
                            digest: header.digest,
                        })
                        .await
                        .map_err(|error| {
                            format!(
                                "resolve committed original {} after Push: {error}",
                                header.id.as_uuid()
                            )
                        })?,
                )
            };
            envelopes.push(envelope);
        }
        envelopes.sort_by_key(|envelope| envelope.id().as_uuid());
        Ok(envelopes)
    }
}
