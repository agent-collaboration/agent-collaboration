//! Poll readiness and push preparation borrow the same physical owner as SDK custody.
use super::{
    AtomicStore, Clock, DeliveryItem, Inbox, ItemId, LocalMailTask, MailDeliveryRuntime,
    RouterDeliveryTypes, StoreError, WakeScheduler,
};
use collaboration_mail_protocol::NotificationSource;
use collaboration_protocol::{PushHeaderFacts, PushRecord};
use message_board::{PendingRootNotice, SubscriptionBatch};

impl MailDeliveryRuntime {
    pub(crate) fn poll_handover_batch(
        item: &DeliveryItem<RouterDeliveryTypes>,
    ) -> Result<SubscriptionBatch, StoreError> {
        let invalid = || StoreError::backend("invalid frozen Poll batch");
        let board_sync_protocol::BoardDeliveryBody::Notification(body) = item.body() else {
            return Err(invalid());
        };
        let NotificationSource::FollowBatch {
            roots,
            policy,
            draining,
            ..
        } = &body.source
        else {
            return Err(invalid());
        };
        if policy.mode() != message_board::SubscriptionMode::Poll || roots.len() != 1 {
            return Err(invalid());
        }
        Ok(SubscriptionBatch {
            batch_id: item
                .id()
                .as_uuid()
                .to_string()
                .try_into()
                .map_err(StoreError::backend)?,
            held: false,
            held_since: None,
            draining: *draining,
            roots: roots
                .iter()
                .map(|range| PendingRootNotice {
                    thread_id: range.root().clone(),
                    project_id: body.pointer.project.clone(),
                    from: range.from(),
                    through: range.through(),
                    post_count: range.post_count(),
                })
                .collect(),
        })
    }
    pub(crate) async fn release_poll_mail(&self, item: ItemId) -> Result<(), StoreError> {
        let config = self.config;
        let now = self.host.now();
        self.store
            .transaction(move |scope| {
                Box::pin(async move {
                    if interaction_storage::read_pending_poll_in_scope(scope, item)
                        .await
                        .map_err(StoreError::backend)?
                        .is_some()
                    {
                        // A newly registered waiter may race the first SDK attempt. None
                        // means no new operation, not an uncertain external handover.
                        let released = Inbox::<_, RouterDeliveryTypes>::new(
                            scope,
                            config.owner,
                            config.limits,
                        )
                        .release_held::<LocalMailTask>(item, now)
                        .await
                        .map_err(StoreError::backend)?;
                        tracing::trace!(
                            ?item,
                            rescheduled = released.is_some(),
                            "Poll waiter requested SDK readiness"
                        );
                    }
                    Ok(())
                })
            })
            .await?;
        self.host.wake_at(now).await.map_err(StoreError::backend)
    }

    pub(crate) async fn prepare_poll_handover(
        &self,
        item: Box<DeliveryItem<RouterDeliveryTypes>>,
    ) -> Result<(PushRecord, SubscriptionBatch), StoreError> {
        let batch = Self::poll_handover_batch(&item)?;
        self.store
            .transaction(move |scope| {
                Box::pin(async move {
                    let invalid = || StoreError::backend("invalid frozen Poll handover");
                    let pending = interaction_storage::read_pending_poll_in_scope(scope, item.id())
                        .await
                        .map_err(StoreError::backend)?
                        .ok_or_else(invalid)?;
                    let board_sync_protocol::BoardDeliveryBody::Notification(body) = item.body()
                    else {
                        return Err(invalid());
                    };
                    let NotificationSource::FollowBatch { membership, .. } = &body.source else {
                        return Err(invalid());
                    };
                    let draft = crate::received_mail_push::received_poll_mail_push(
                        &item,
                        pending.received_at,
                    )
                    .map_err(StoreError::backend)?;
                    if draft.target != pending.recipient
                        || membership.as_str() != pending.membership.as_str()
                    {
                        return Err(invalid());
                    }
                    if let Some(current) =
                        interaction_storage::read_mail_push_in_scope(scope, &draft.push_id)
                            .await
                            .map_err(StoreError::backend)?
                    {
                        let mut header = current.header_facts.clone();
                        if let PushHeaderFacts::SubscriptionActivity { held_since, .. } =
                            &mut header
                        {
                            *held_since = None;
                        }
                        let mut activity = current.activity.clone();
                        if let Some(activity) = &mut activity {
                            activity.held = false;
                        }
                        if current.target != draft.target
                            || current.kind != draft.kind
                            || current.created_at != draft.created_at
                            || current.origin_router_ref != draft.origin_router_ref
                            || header != draft.header_facts
                            || activity != draft.activity
                        {
                            return Err(invalid());
                        }
                    } else {
                        interaction_storage::insert_mail_push_in_scope(scope, draft.clone())
                            .await
                            .map_err(StoreError::backend)?;
                    }
                    let record = interaction_storage::mark_mail_push_attempted_in_scope(
                        scope,
                        &draft.push_id,
                    )
                    .await
                    .map_err(StoreError::backend)?;
                    Ok((record, batch))
                })
            })
            .await
    }
}
