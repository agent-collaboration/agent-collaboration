//! Two authenticated sessions and literal envelopes for mailbox rule cases.
use board_sync_protocol::BoardDeliveryTypes;
use collaboration_central_conformance::{
    AdmittedPeerSocket, ConformanceClient, ConformanceResult, enroll_admitted_peer,
};
use fleet_sync_protocol::{DeliveryEnvelope, DeliveryItem, ItemId, PeerId};
use serde_json::{Value, json};

pub(super) const ORIGIN: &str = "550e8400-e29b-41d4-a716-446655441081";
pub(super) const DESTINATION: &str = "550e8400-e29b-41d4-a716-446655441082";
pub(super) const ORIGINAL: &str = "019f0000-0000-7000-8000-000000011081";

pub(super) struct MailboxPair {
    pub sender: AdmittedPeerSocket,
    pub recipient: AdmittedPeerSocket,
}
impl MailboxPair {
    pub async fn open(base: &str) -> ConformanceResult<Self> {
        let client = ConformanceClient::new(base)?;
        let sender = enroll_admitted_peer(
            &client,
            ORIGIN,
            "019f0000-0000-7000-8000-000000001081",
            "mailbox-rule-sender",
            "019f0000-0000-7000-8000-000000021081",
        )
        .await?;
        let recipient = enroll_admitted_peer(
            &client,
            DESTINATION,
            "019f0000-0000-7000-8000-000000001082",
            "mailbox-rule-recipient",
            "019f0000-0000-7000-8000-000000021082",
        )
        .await?;
        Ok(Self { sender, recipient })
    }
    pub fn item(
        &self,
        id: &str,
        text: &str,
    ) -> ConformanceResult<DeliveryEnvelope<BoardDeliveryTypes>> {
        Ok(DeliveryEnvelope::Item(DeliveryItem::new(
            ItemId::try_from(id.to_owned())?,
            serde_json::from_value(json!({"kind":"humanUser","installation":ORIGIN}))?,
            serde_json::from_value(json!({"peer":PeerId::new(self.recipient.peer)?,"address":{
                "endpoint":{"serviceId":DESTINATION,"endpointId":"codex-local"},"sessionId":"rule recipient"
            }}))?,
            None,
            serde_json::from_value(
                json!({"kind":"message","text":text,"delivery":"auto","guard":null,"senderDisplayName":null,"replyTo":null}),
            )?,
        )?))
    }
    pub async fn submit(
        &mut self,
        item: &DeliveryEnvelope<BoardDeliveryTypes>,
    ) -> ConformanceResult<Value> {
        super::relay_transport::operation(&mut self.sender.socket,
            json!({"kind":"consumer","value":{"kind":"mailSubmit","value":{"item":item,"originEpoch":null}}})).await
    }
    pub fn cancel(&self, id: &str) -> ConformanceResult<DeliveryEnvelope<BoardDeliveryTypes>> {
        use fleet_sync_protocol::{CancelBody, DeliveryCancel};
        let original = self.item(ORIGINAL, "literal original mailbox body")?;
        Ok(DeliveryEnvelope::Cancel(DeliveryCancel::new(
            ItemId::try_from(id.to_owned())?,
            original.sender().clone(),
            original.recipient().clone(),
            None,
            CancelBody {
                target: ItemId::try_from(ORIGINAL.to_owned())?,
            },
        )?))
    }
    pub async fn close(self) -> ConformanceResult<()> {
        let sender = self
            .sender
            .socket
            .close(reqwest_websocket::CloseCode::Normal, None)
            .await;
        let recipient = self
            .recipient
            .socket
            .close(reqwest_websocket::CloseCode::Normal, None)
            .await;
        sender?;
        recipient?;
        Ok(())
    }
}

pub(super) async fn control(
    base: &str,
    address: Value,
    command: Value,
) -> ConformanceResult<Value> {
    Ok(reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(8))
        .build()?
        .post(format!("{base}/admin/v1/conformance/runtime"))
        .header(
            "Cf-Access-Jwt-Assertion",
            collaboration_central_conformance::owner_assertion()?,
        )
        .json(&json!({"address":address,"command":command}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}
