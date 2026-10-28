//! Submit exactly one SDK Poll callback to its session's existing reader owner.
use super::super::reader_delivery_owner::poll_unsent;
use super::{
    ReaderDeliveryCommand, SessionRef, SubscriptionDeliveryService, oneshot, reader_for_target,
};

impl SubscriptionDeliveryService {
    pub(crate) async fn attempt_poll_mail_once(
        &self,
        item: Box<fleet_sync_protocol::DeliveryItem<collaboration_contracts::RouterDeliveryTypes>>,
    ) -> collaboration_contracts::NativeDeliveryResult {
        let target: SessionRef = match item.recipient().address.clone().try_into() {
            Ok(target) => target,
            Err(_) => return poll_unsent("invalid Poll recipient"),
        };
        if target.endpoint.service_id != *self.inner.push.machine.service_id()
            || self.require_started().await.is_err()
        {
            return poll_unsent("Poll reader carrier is unavailable before handover");
        }
        let reader = match reader_for_target(&target) {
            Ok(reader) => reader,
            Err(_) => return poll_unsent("invalid Poll reader"),
        };
        let sender = match self.ensure_owner(reader).await {
            Ok(sender) => sender,
            Err(_) => return poll_unsent("Poll reader closed before handover"),
        };
        let (reply, response) = oneshot::channel();
        let sent = tokio::select! {
            () = self.inner.shutdown.cancelled() => return poll_unsent("Poll reader closed before accepting handover"),
            result = sender.send(ReaderDeliveryCommand::PollMailAttempt { item, reply }) => result,
        };
        if sent.is_err() {
            return poll_unsent("Poll reader closed before accepting handover");
        }
        // A lost answer after the reader accepted the external handover request
        // stays Unknown. It must never be turned into a second waiter delivery.
        tokio::select! {
            () = self.inner.shutdown.cancelled() => collaboration_contracts::NativeDeliveryResult::unknown(),
            outcome = response => outcome.unwrap_or_else(|_| collaboration_contracts::NativeDeliveryResult::unknown()),
        }
    }
}
