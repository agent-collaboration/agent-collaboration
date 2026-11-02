//! The reader owns one rolling notification window; immediate message kinds bypass it.
use super::super::subscription_push::target_session;
use super::ReaderDeliveryOwner;
use collaboration_contracts::layer_zero::{DeliveryRequest, PreparedPush};
use collaboration_contracts::{
    DeliveryPrecondition, LoadPolicy, NativeDeliveryResult, NativeRefusalCode, UnsentCause,
};
use collaboration_protocol::{
    DeliveryCorrelationId, DeliveryOutcome, MachineId, MessageDelivery, MessageText, PushId,
    PushKind, PushLineInput, PushRecord, PushRecordDraft, RouterLink, render_push_line,
};
use message_board::BoardError;

fn unavailable(_: interaction_storage::StorageError) -> BoardError {
    BoardError::board_unavailable()
}
fn held_notification() -> NativeDeliveryResult {
    NativeDeliveryResult::not_submitted(
        "notification waits for the recipient's rolling delivery window",
        true,
        UnsentCause::Holdable {
            otherwise: NativeRefusalCode::CarrierUnavailable,
        },
    )
}
fn receipt_consumes_window(receipt: &collaboration_protocol::DeliveryReceipt) -> bool {
    !matches!(
        receipt.outcome,
        DeliveryOutcome::Rejected(_) | DeliveryOutcome::NotSubmitted { .. }
    )
}
impl ReaderDeliveryOwner {
    pub(super) async fn ensure_notification_recovery(&mut self) -> Result<(), BoardError> {
        if !self.notification_recovery_complete {
            if let Ok(target) = target_session(&self.reader) {
                self.push
                    .store
                    .recover_interrupted_notification_listings(
                        &target,
                        &self.notification_startup_attempts,
                        self.clock.now(),
                    )
                    .await
                    .map_err(unavailable)?;
            }
            self.notification_recovery_complete = true;
        }
        Ok(())
    }

    pub(super) async fn deliver_stored_notification(
        &mut self,
        record: &PushRecord,
    ) -> Result<PushRecord, BoardError> {
        self.ensure_notification_recovery().await?;
        let line = render_push_line(&PushLineInput {
            link: RouterLink::new(
                MachineId::from(self.push.machine.service_id().clone()),
                record.push_id.clone(),
            ),
            machine_label: self.push.machine.machine_label().clone(),
            origin: record.origin.clone(),
            header_facts: record.header_facts.clone(),
            // Subscription bodies remain stored for show; native subscription
            // lines carry the reference without previewing the renewal command.
            body: None,
        })
        .map_err(|_| BoardError::board_unavailable())?;
        let prepared = PreparedPush {
            push_id: record.push_id.clone(),
            line: MessageText::try_from(line).map_err(|_| BoardError::board_unavailable())?,
            load_policy: LoadPolicy::LoadedOnly,
        };
        let request = DeliveryRequest {
            target: record.target.clone(),
            payload: prepared.clone(),
            mode: MessageDelivery::Auto,
            precondition: DeliveryPrecondition::Unpinned,
            correlation: DeliveryCorrelationId::try_from(record.push_id.as_str().to_owned())
                .map_err(|_| BoardError::board_unavailable())?,
            attempt: agent_automation::AttemptId::generate(),
        };
        self.push.mark_attempted(&prepared).await?;
        let result = self.attempt_notification(record.kind, request).await?;
        self.push
            .record_receipt(&prepared, result.receipt(), true)
            .await?;
        self.push.direct_message(&record.push_id).await
    }

    pub(super) async fn attempt_notification(
        &mut self,
        kind: PushKind,
        request: DeliveryRequest,
    ) -> Result<NativeDeliveryResult, BoardError> {
        if let Err(error) = self.ensure_notification_recovery().await {
            self.record_pass_error(&error);
            return Ok(NativeDeliveryResult::not_submitted(
                "recipient recovery has not completed; no native attempt was made",
                true,
                UnsentCause::Holdable {
                    otherwise: NativeRefusalCode::CarrierUnavailable,
                },
            ));
        }
        if !interaction_storage::is_notification_kind(kind) {
            return self.push.dispatch_mail_attempt(kind, request).await;
        }
        if let Some(listing) = self
            .push
            .store
            .notification_listing_for_member(&request.payload.push_id)
            .await
            .map_err(unavailable)?
        {
            return Ok(crate::mail_push_outcome::listing_native_result(&listing));
        }
        let now = self.clock.now();
        let window = self
            .push
            .store
            .notification_window(&request.target)
            .await
            .map_err(unavailable)?;
        if let Some(window) = window
            && (window.flush_due_at.is_some()
                || now < window.next_delivery_at().map_err(unavailable)?)
        {
            let due = self
                .push
                .store
                .hold_notification_window(&request.target)
                .await
                .map_err(unavailable)?;
            self.notification_flush_deadline = Some(
                crate::subscription_delivery::subscription_facts::wall_deadline(
                    due,
                    now,
                    self.clock.monotonic_now(),
                ),
            );
            return Ok(held_notification());
        }
        let target = request.target.clone();
        let delivery = self.push.dispatch_mail_attempt(kind, request).await?;
        if receipt_consumes_window(delivery.receipt()) {
            // After native I/O retry only the durable receipt/window write, never the effect.
            let delivered_at = self.clock.now();
            loop {
                match self
                    .push
                    .store
                    .record_notification_delivery(&target, delivered_at)
                    .await
                {
                    Ok(()) => break,
                    Err(error) => {
                        tracing::warn!(%error,"notification window write failed after native effect");
                        let deadline = self.retry_deadline();
                        tokio::select! {() = self.shutdown.cancelled() =>return Err(BoardError::board_unavailable()),() = self.clock.sleep_until(deadline) =>{}}
                    }
                }
            }
        }
        Ok(delivery)
    }
    pub(super) async fn flush_due_notifications(&mut self) -> Result<bool, BoardError> {
        let Ok(target) = target_session(&self.reader) else {
            return Ok(false);
        };
        if self
            .notification_flush_deadline
            .is_some_and(|deadline| self.clock.monotonic_now() < deadline)
        {
            return Ok(true);
        }
        let mail = self.native_mail.get().and_then(std::sync::Weak::upgrade);
        let pending = self
            .push
            .store
            .pending_notification_listings(&target)
            .await
            .map_err(unavailable)?;
        if pending.len() > 1 {
            return Err(BoardError::board_unavailable());
        }
        if let Some(listing) = pending.into_iter().next() {
            self.deliver_notification_listing(listing, mail.as_deref())
                .await?;
            return Ok(true);
        }
        let mut window = self
            .push
            .store
            .notification_window(&target)
            .await
            .map_err(unavailable)?;
        if window
            .as_ref()
            .is_some_and(|window| window.flush_due_at.is_none())
            && !self
                .push
                .store
                .held_notification_candidates(&target)
                .await
                .map_err(unavailable)?
                .is_empty()
        {
            self.push
                .store
                .hold_notification_window(&target)
                .await
                .map_err(unavailable)?;
            window = self
                .push
                .store
                .notification_window(&target)
                .await
                .map_err(unavailable)?;
        }
        self.notification_flush_deadline = window
            .as_ref()
            .and_then(|window| window.flush_due_at)
            .map(|due| {
                crate::subscription_delivery::subscription_facts::wall_deadline(
                    due,
                    self.clock.now(),
                    self.clock.monotonic_now(),
                )
            });
        let Some(window) = window else {
            return Ok(false);
        };
        let Some(due) = window.flush_due_at else {
            return Ok(false);
        };
        if self.clock.now() < due {
            return Ok(true);
        }
        let candidates = self
            .push
            .store
            .held_notification_candidates(&target)
            .await
            .map_err(unavailable)?;
        if candidates.is_empty() {
            self.push
                .store
                .clear_notification_flush(&target)
                .await
                .map_err(unavailable)?;
            self.notification_flush_deadline = None;
            return Ok(false);
        }
        let mut eligible = Vec::new();
        let mut load_policy = LoadPolicy::LoadedOnly;
        for record in candidates {
            // Failed-listing SDK callbacks must consume their frozen result
            // before the original can join a new listing, including after reopen.
            if self
                .push
                .store
                .notification_listing_for_member(&record.push_id)
                .await
                .map_err(unavailable)?
                .is_some()
            {
                continue;
            }
            let reference = self
                .push
                .store
                .notification_item_reference(&record.push_id)
                .await
                .map_err(unavailable)?;
            if let Some(reference) = reference {
                let Some(mail) = &mail else {
                    continue;
                };
                let (item, preparation) = mail
                    .prepare_notification_for_listing(reference.clone())
                    .await
                    .map_err(|_| BoardError::board_unavailable())?;
                match preparation {
                    collaboration_runtime::MailPushPreparation::Attempt { request, .. } => {
                        if self
                            .notification_follow_prevented(&item, &reference, &target)
                            .await?
                        {
                            continue;
                        }
                        if request.payload.load_policy == LoadPolicy::MayLoad {
                            load_policy = LoadPolicy::MayLoad;
                        }
                    }
                    collaboration_runtime::MailPushPreparation::Drop { .. }
                    | collaboration_runtime::MailPushPreparation::Refuse { .. } => {
                        mail.release_held_mail(vec![record.push_id])
                            .await
                            .map_err(|_| BoardError::board_unavailable())?;
                        continue;
                    }
                    collaboration_runtime::MailPushPreparation::Hold { .. } => continue,
                }
            } else if !matches!(
                self.presence.presence(&target).await,
                Ok(collaboration_contracts::TargetPresence::Running)
            ) {
                continue;
            }
            eligible.push(record);
        }
        if eligible.is_empty() {
            self.notification_flush_deadline = Some(
                self.clock.monotonic_now()
                    + crate::subscription_delivery::subscription_facts::PRESENCE_INTERVAL,
            );
            return Ok(true);
        }
        let draft = self.notification_listing_draft(&target, &eligible)?;
        let members = eligible
            .iter()
            .map(|record| record.push_id.clone())
            .collect::<Vec<_>>();
        let listing = self
            .push
            .store
            .freeze_notification_listing(draft, &members)
            .await
            .map_err(unavailable)?;
        self.deliver_listing_with_policy(listing, mail.as_deref(), load_policy)
            .await?;
        Ok(true)
    }
    async fn notification_follow_prevented(
        &self,
        item: &fleet_sync_protocol::DeliveryItem<collaboration_contracts::RouterDeliveryTypes>,
        reference: &fleet_sync_protocol::DeliveryItemRef,
        target: &collaboration_protocol::SessionRef,
    ) -> Result<bool, BoardError> {
        let board_sync_protocol::BoardDeliveryBody::Notification(body) = item.body() else {
            return Ok(false);
        };
        if !matches!(
            body.source,
            collaboration_mail_protocol::NotificationSource::FollowBatch { .. }
        ) {
            return Ok(false);
        }
        let collaboration_mail_protocol::MailSender::Project { project } = item.sender() else {
            return Err(BoardError::board_unavailable());
        };
        let publisher = self
            .follow_admission
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .and_then(|port| port.publisher.upgrade())
            .ok_or_else(BoardError::board_unavailable)?;
        publisher
            .before_local_follow_injection(project, reference, target)
            .await
    }
    fn notification_listing_draft(
        &self,
        target: &collaboration_protocol::SessionRef,
        members: &[PushRecord],
    ) -> Result<PushRecordDraft, BoardError> {
        let id = PushId::try_from(uuid::Uuid::now_v7().to_string())
            .map_err(|_| BoardError::board_unavailable())?;
        crate::notification_listing_content::draft_for_members(
            MachineId::from(self.push.machine.service_id().clone()),
            target,
            id,
            members,
            self.clock.now(),
        )
    }
    async fn deliver_notification_listing(
        &mut self,
        listing: PushRecord,
        mail: Option<&crate::MailDeliveryRuntime>,
    ) -> Result<(), BoardError> {
        // Pending recovered listings must recompute native eligibility from the SDK's frozen policies.
        let mut policy = LoadPolicy::LoadedOnly;
        let mut after = None;
        loop {
            let (members, next) = self
                .push
                .store
                .notification_listing_members(&listing.target, &listing.push_id, after.as_ref(), 50)
                .await
                .map_err(unavailable)?;
            for record in members {
                if let Some(reference) = self
                    .push
                    .store
                    .notification_item_reference(&record.push_id)
                    .await
                    .map_err(unavailable)?
                {
                    let Some(mail) = mail else {
                        return Err(BoardError::board_unavailable());
                    };
                    let (item, preparation) = mail
                        .prepare_notification_for_listing(reference.clone())
                        .await
                        .map_err(|_| BoardError::board_unavailable())?;
                    if self
                        .notification_follow_prevented(&item, &reference, &listing.target)
                        .await?
                    {
                        return self
                            .reject_pending_listing(
                                &listing,
                                "follow was prevented before recovered listing injection",
                            )
                            .await;
                    }
                    match preparation {
                        collaboration_runtime::MailPushPreparation::Attempt { request, .. } => {
                            if request.payload.load_policy == LoadPolicy::MayLoad {
                                policy = LoadPolicy::MayLoad;
                            }
                        }
                        _ => {
                            return self
                                .reject_pending_listing(
                                    &listing,
                                    "listing recipient policy is not currently eligible",
                                )
                                .await;
                        }
                    }
                }
            }
            match next {
                Some(next) => after = Some(next),
                None => break,
            }
        }
        self.deliver_listing_with_policy(listing, mail, policy)
            .await
    }
    async fn reject_pending_listing(
        &self,
        listing: &PushRecord,
        reason: &str,
    ) -> Result<(), BoardError> {
        self.push
            .store
            .mark_push_attempted(&listing.push_id)
            .await
            .map_err(unavailable)?;
        let members = self
            .push
            .store
            .settle_notification_listing(
                &listing.push_id,
                held_notification().into_receipt(),
                self.clock.now(),
            )
            .await
            .map_err(unavailable)?;
        if let Some(mail) = self.native_mail.get().and_then(std::sync::Weak::upgrade) {
            // A reservation rollback admitted no callbacks. Release the public
            // Held items so they consume this known-unsent receipt and unlink.
            mail.release_held_mail(members)
                .await
                .map_err(|_| BoardError::board_unavailable())?;
            mail.wake_notification_callbacks()
                .await
                .map_err(|_| BoardError::board_unavailable())?;
        }
        tracing::debug!(%reason,"notification listing was not sent; members released");
        Ok(())
    }
    async fn deliver_listing_with_policy(
        &mut self,
        mut listing: PushRecord,
        mail: Option<&crate::MailDeliveryRuntime>,
        mut load_policy: LoadPolicy,
    ) -> Result<(), BoardError> {
        if let Some(mail) = mail {
            match mail.reserve_notification_listing(&listing).await {
                Ok(Some(reserved)) => {
                    let changed = reserved.header_facts != listing.header_facts;
                    listing = reserved;
                    if changed {
                        load_policy = self.refrozen_listing_load_policy(&listing).await?;
                    }
                }
                Ok(None) => {
                    mail.wake_notification_callbacks()
                        .await
                        .map_err(|_| BoardError::board_unavailable())?;
                    self.notification_flush_deadline = Some(
                        self.clock.monotonic_now()
                            + crate::subscription_delivery::subscription_facts::PRESENCE_INTERVAL,
                    );
                    return Ok(());
                }
                Err(error) => {
                    tracing::warn!(%error,"listing reservation made no native call");
                    return self
                        .reject_pending_listing(&listing, "notification reservation refused")
                        .await;
                }
            }
        } else {
            self.push
                .store
                .mark_push_attempted(&listing.push_id)
                .await
                .map_err(unavailable)?;
        }
        let line = render_push_line(&PushLineInput {
            link: RouterLink::new(
                MachineId::from(self.push.machine.service_id().clone()),
                listing.push_id.clone(),
            ),
            machine_label: self.push.machine.machine_label().clone(),
            origin: listing.origin.clone(),
            header_facts: listing.header_facts.clone(),
            body: listing.body.clone(),
        })
        .map_err(|_| BoardError::board_unavailable())?;
        let request = DeliveryRequest {
            target: listing.target.clone(),
            payload: PreparedPush {
                push_id: listing.push_id.clone(),
                line: MessageText::try_from(line).map_err(|_| BoardError::board_unavailable())?,
                load_policy,
            },
            mode: MessageDelivery::Auto,
            precondition: DeliveryPrecondition::Unpinned,
            correlation: DeliveryCorrelationId::try_from(listing.push_id.as_str().to_owned())
                .map_err(|_| BoardError::board_unavailable())?,
            attempt: agent_automation::AttemptId::generate(),
        };
        let result = self
            .push
            .dispatch_mail_attempt(PushKind::NotificationListing, request)
            .await
            .unwrap_or_else(|_| NativeDeliveryResult::unknown());
        let delivered_at = self.clock.now();
        loop {
            match self
                .push
                .store
                .settle_notification_listing(
                    &listing.push_id,
                    result.receipt().clone(),
                    delivered_at,
                )
                .await
            {
                Ok(_) => break,
                Err(error) => {
                    tracing::warn!(%error,"listing receipt write failed after native effect; retaining the same receipt");
                    let deadline = self.retry_deadline();
                    tokio::select! {() = self.shutdown.cancelled() =>return Err(BoardError::board_unavailable()),() = self.clock.sleep_until(deadline) =>{}}
                }
            }
        }
        if let Some(mail) = mail {
            mail.wake_notification_callbacks()
                .await
                .map_err(|_| BoardError::board_unavailable())?;
        }
        self.notification_flush_deadline = if receipt_consumes_window(result.receipt()) {
            None
        } else {
            Some(
                self.clock.monotonic_now()
                    + crate::subscription_delivery::subscription_facts::PRESENCE_INTERVAL,
            )
        };
        Ok(())
    }

    async fn refrozen_listing_load_policy(
        &self,
        listing: &PushRecord,
    ) -> Result<LoadPolicy, BoardError> {
        let mail = self
            .native_mail
            .get()
            .and_then(std::sync::Weak::upgrade)
            .ok_or_else(BoardError::board_unavailable)?;
        let mut policy = LoadPolicy::LoadedOnly;
        let mut after = None;
        loop {
            let (members, next) = self
                .push
                .store
                .notification_listing_members(&listing.target, &listing.push_id, after.as_ref(), 50)
                .await
                .map_err(unavailable)?;
            for member in members {
                if let Some(reference) = self
                    .push
                    .store
                    .notification_item_reference(&member.push_id)
                    .await
                    .map_err(unavailable)?
                {
                    let (_, preparation) = mail
                        .prepare_notification_for_listing(reference)
                        .await
                        .map_err(|_| BoardError::board_unavailable())?;
                    if matches!(preparation, collaboration_runtime::MailPushPreparation::Attempt { request, .. } if request.payload.load_policy == LoadPolicy::MayLoad)
                    {
                        policy = LoadPolicy::MayLoad;
                    }
                }
            }
            match next {
                Some(next) => after = Some(next),
                None => break,
            }
        }
        Ok(policy)
    }
}
