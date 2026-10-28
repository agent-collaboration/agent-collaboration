//! A registered wait probes FIFO SDK items and hands over one matching frozen root.
use super::{
    BoardError, Identity, ReaderDeliveryOwner, ReaderScanScope, SubscriptionMode,
    SubscriptionWaitResult, target_session,
};
#[cfg(test)]
use crate::subscription_delivery::subscription_service::OwnerObservation;
use collaboration_mail_protocol::NotificationSource;
#[cfg(test)]
use collaboration_protocol::PushId;
use collaboration_protocol::{MachineId, PushLineInput, RouterLink, render_push_line};
use fleet_sync_protocol::DeliveryItem;

pub(in crate::subscription_delivery) fn poll_unsent(
    reason: &str,
) -> collaboration_contracts::NativeDeliveryResult {
    collaboration_contracts::NativeDeliveryResult::not_submitted(
        reason,
        true,
        collaboration_contracts::UnsentCause::Holdable {
            otherwise: collaboration_contracts::NativeRefusalCode::CarrierUnavailable,
        },
    )
}

impl ReaderDeliveryOwner {
    #[cfg(test)]
    async fn pause_poll_handover_for_test(
        &self,
        push: &PushId,
        stage: super::super::subscription_service::PollHandoverStage,
    ) {
        let checkpoint = {
            let mut slot = self.poll_handover_probe.lock().unwrap();
            if slot
                .as_ref()
                .is_some_and(|probe| &probe.push == push && probe.stage == stage)
            {
                slot.take()
            } else {
                None
            }
        };
        if let Some(checkpoint) = checkpoint {
            let _ = checkpoint.entered.send(());
            let _ = checkpoint.resume.await;
        }
    }
    pub(super) async fn resume_poll_waiters(&mut self) -> Result<(), BoardError> {
        if self.waiters.is_empty() || !matches!(self.reader, Identity::Session { .. }) {
            return Ok(());
        }
        let Some(runtime) = self.native_mail.get().and_then(std::sync::Weak::upgrade) else {
            return Err(BoardError::board_unavailable());
        };
        let rows = self
            .push
            .store
            .pending_poll_mail(&target_session(&self.reader)?)
            .await
            .map_err(|_| BoardError::board_unavailable())?;
        for waiter in &mut self.waiters {
            if let Some(item) = waiter.poll_in_flight {
                if rows.iter().any(|row| row.item == item) {
                    return Ok(());
                }
                waiter.poll_in_flight = None;
            }
            let Some(next) = rows
                .iter()
                .find(|row| !waiter.poll_filter_mismatches.contains(&row.item))
            else {
                continue;
            };
            waiter.poll_in_flight = Some(next.item);
            tracing::trace!(item = ?next.item, "Poll waiter reserved its next FIFO candidate");
            runtime
                .release_poll_mail(next.item)
                .await
                .map_err(|_| BoardError::board_unavailable())?;
            // Each SDK callback exposes its frozen filter facts. Do not let a
            // later waiter overtake this request while those facts are pending.
            return Ok(());
        }
        Ok(())
    }

    pub(super) async fn hand_over_poll_mail(
        &mut self,
        item: Box<DeliveryItem<collaboration_contracts::RouterDeliveryTypes>>,
    ) -> collaboration_contracts::NativeDeliveryResult {
        tracing::trace!(item = ?item.id(), waiters = self.waiters.len(), "SDK Poll callback reached its reader");
        let Some(index) = self
            .waiters
            .iter()
            .position(|waiter| waiter.poll_in_flight == Some(item.id()))
        else {
            return poll_unsent("Poll item waits for a matching waiter");
        };
        let Some(waiter) = self.waiters.get_mut(index) else {
            return poll_unsent("Poll waiter disappeared before checking its filter");
        };
        waiter.poll_in_flight = None;
        let board_sync_protocol::BoardDeliveryBody::Notification(body) = item.body() else {
            return poll_unsent("invalid Poll notification");
        };
        let NotificationSource::FollowBatch { roots, policy, .. } = &body.source else {
            return poll_unsent("invalid Poll source");
        };
        if policy.mode() != SubscriptionMode::Poll || roots.len() != 1 {
            return poll_unsent("invalid frozen Poll shape");
        }
        let Some(root) = roots.first() else {
            return poll_unsent("Poll item has no frozen root");
        };
        if !waiter.filter.matches(root.root(), &body.pointer.project) {
            waiter.poll_filter_mismatches.insert(item.id());
            self.request_scan(ReaderScanScope::All);
            return poll_unsent("Poll item does not match this waiter");
        }
        let notice_bytes = crate::MailDeliveryRuntime::poll_handover_batch(&item)
            .ok()
            .and_then(|batch| serde_json::to_vec(&batch.roots).ok());
        if notice_bytes.is_none_or(|bytes| bytes.len() > waiter.maximum_root_notice_bytes) {
            if let Some(waiter) = self.waiters.remove(index) {
                let _ = waiter.reply.send(Err(BoardError::invalid_field(
                    "byteBudget",
                    "wait budget must fit one frozen root",
                )));
            }
            return poll_unsent("Poll notice exceeds the wait budget");
        }
        let reference = fleet_sync_protocol::DeliveryItemRef {
            item: item.id(),
            digest: item.digest().clone(),
        };
        let target = match target_session(&self.reader) {
            Ok(target) => target,
            Err(_) => return poll_unsent("Poll target unavailable"),
        };
        let publisher = self
            .follow_admission
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .and_then(|port| port.publisher.upgrade());
        let Some(publisher) = publisher else {
            #[cfg(test)]
            self.observe(OwnerObservation::PollPublisherUnavailable(item.id()));
            return poll_unsent("Poll item waits for its cancellation checker");
        };
        if !matches!(
            publisher
                .before_local_follow_injection(&body.pointer.project, &reference, &target)
                .await,
            Ok(false)
        ) {
            #[cfg(test)]
            self.observe(OwnerObservation::PollCancellationDeferred(item.id()));
            return poll_unsent("Poll item waits for its cancellation check");
        }
        let Some(runtime) = self.native_mail.get().and_then(std::sync::Weak::upgrade) else {
            return poll_unsent("Poll runtime unavailable before handover");
        };
        let prepared = runtime.prepare_poll_handover(item).await;
        let (record, batch) = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                tracing::warn!(%error, "Poll push preparation failed before handover");
                if let Some(waiter) = self.waiters.remove(index) {
                    let _ = waiter.reply.send(Err(BoardError::board_unavailable()));
                }
                return poll_unsent("Poll push preparation failed before handover");
            }
        };
        tracing::trace!(
            push_id = record.push_id.as_str(),
            "Poll handover push is prepared"
        );
        #[cfg(test)]
        self.pause_poll_handover_for_test(
            &record.push_id,
            super::super::subscription_service::PollHandoverStage::BeforeSend,
        )
        .await;
        let Some(waiter) = self.waiters.remove(index) else {
            return poll_unsent("Poll waiter disappeared before handover");
        };
        if waiter.deadline <= self.clock.monotonic_now() || waiter.reply.is_closed() {
            let _ = waiter.reply.send(Ok(None));
            return poll_unsent("wait caller disconnected before handover");
        }
        let line = render_push_line(&PushLineInput {
            link: RouterLink::new(
                MachineId::from(self.push.machine.service_id().clone()),
                record.push_id.clone(),
            ),
            machine_label: self.push.machine.machine_label().clone(),
            origin: record.origin,
            header_facts: record.header_facts,
            body: None,
        })
        .ok()
        .and_then(|line| line.try_into().ok());
        let Some(line) = line else {
            let _ = waiter.reply.send(Err(BoardError::board_unavailable()));
            return poll_unsent("Poll notice could not be rendered");
        };
        #[cfg(test)]
        let observed_push = record.push_id.clone();
        if waiter
            .reply
            .send(Ok(Some(SubscriptionWaitResult::Notice {
                push_id: record.push_id,
                line,
                batch,
            })))
            .is_err()
        {
            return poll_unsent("wait caller disconnected before handover");
        }
        #[cfg(test)]
        self.pause_poll_handover_for_test(
            &observed_push,
            super::super::subscription_service::PollHandoverStage::AfterSend,
        )
        .await;
        collaboration_contracts::NativeDeliveryResult::observed(
            collaboration_contracts::NativeObservedOutcome::Started,
            None,
            None,
        )
    }
}
