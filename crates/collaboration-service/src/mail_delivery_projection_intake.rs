//! Real Inbox intake and committed outcome observations for the projection fixture.
use super::*;

impl Fixture {
    pub async fn receive(&self, draft: PushRecordDraft) -> Result<(), StoreError> {
        let item = self.item(&draft);
        self.receive_items(vec![item]).await
    }
    pub async fn receive_items(
        &self,
        items: Vec<DeliveryItem<RouterDeliveryTypes>>,
    ) -> Result<(), StoreError> {
        self.receive_envelopes(items.into_iter().map(DeliveryEnvelope::Item).collect())
            .await
            .map(|_| ())
    }
    pub async fn push_item(
        &self,
        item: DeliveryItem<RouterDeliveryTypes>,
    ) -> Result<PushResult<CarrierAcceptance>, StoreError> {
        self.receive_envelopes(vec![DeliveryEnvelope::Item(item)])
            .await?
            .pop()
            .ok_or_else(|| StoreError::backend("fixture Push receipt absent"))
    }
    async fn receive_envelopes(
        &self,
        items: Vec<DeliveryEnvelope<RouterDeliveryTypes>>,
    ) -> Result<Vec<PushResult<CarrierAcceptance>>, StoreError> {
        let config = self.host.config;
        let binding = self.host.binding.clone();
        let now = self.host.now();
        let consumer = self.host.consumer.clone();
        self.store
            .transaction(move |scope| {
                Box::pin(async move {
                    let mut answers = Vec::with_capacity(items.len());
                    for item in items {
                        let submitter =
                            <crate::MachineSenderBinding as fleet_sync_delivery::SenderBinding<
                                _,
                                RouterDeliveryTypes,
                            >>::resolve_submitter(
                                binding.as_ref(), scope, item.sender()
                            )
                            .await
                            .map_err(|reason| {
                                StoreError::backend(format!("fixture sender binding: {reason:?}"))
                            })?;
                        let answer = Inbox::<_, RouterDeliveryTypes, _>::new(
                            scope,
                            config.local.owner,
                            config.local.limits,
                            binding.as_ref(),
                        )
                        .accept_push::<ProjectionTask, ProjectionReport, ProjectionRetention, _>(
                            submitter,
                            item,
                            config.relay,
                            consumer.as_ref(),
                            now,
                        )
                        .await
                        .map_err(StoreError::backend)?;
                        if !matches!(
                            answer,
                            PushResult::Accepted { .. } | PushResult::Replayed { .. }
                        ) {
                            return Err(StoreError::backend("fixture Push was not admitted"));
                        }
                        answers.push(answer);
                    }
                    Ok(answers)
                })
            })
            .await
    }
    pub async fn receive_cancel(&self, draft: &PushRecordDraft) -> ItemId {
        let original = self.item(draft);
        let control = ItemId::new(uuid::Uuid::now_v7()).unwrap();
        let cancel = fleet_sync_protocol::DeliveryCancel::<RouterDeliveryTypes>::new(
            control,
            original.sender().clone(),
            original.recipient().clone(),
            None,
            fleet_sync_protocol::CancelBody {
                target: original.id(),
            },
        )
        .unwrap();
        self.receive_envelopes(vec![DeliveryEnvelope::Cancel(cancel)])
            .await
            .unwrap();
        control
    }
    pub async fn report(&self, item: ItemId) -> Option<RecipientOutcome<CarrierAcceptance>> {
        let config = self.host.config;
        let binding = self.host.binding.clone();
        self.store
            .transaction(move |scope| {
                Box::pin(async move {
                    let mut inbox = Inbox::<_, RouterDeliveryTypes, _>::new(
                        scope,
                        config.local.owner,
                        config.local.limits,
                        binding.as_ref(),
                    );
                    let Some(reference) = inbox
                        .pending_report(item)
                        .await
                        .map_err(StoreError::backend)?
                    else {
                        return Ok(None);
                    };
                    Ok(Some(
                        inbox
                            .resolve_report(reference)
                            .await
                            .map_err(StoreError::backend)?
                            .outcome,
                    ))
                })
            })
            .await
            .unwrap()
    }
    pub async fn sql(&self, statement: &'static str) {
        self.store
            .transaction(move |scope| {
                Box::pin(async move {
                    sqlx::raw_sql(statement).execute(scope.executor()).await?;
                    Ok(())
                })
            })
            .await
            .unwrap();
    }
    pub async fn counts(&self) -> Vec<i64> {
        self.store
            .transaction(|scope| {
                Box::pin(async move {
                    let mut counts = Vec::new();
                    for statement in [
                        "SELECT COUNT(*) AS count FROM _fleet_sync_inbox_items",
                        "SELECT COUNT(*) AS count FROM router_pushes",
                        "SELECT COUNT(*) AS count FROM _fleet_sync_reports",
                        "SELECT COUNT(*) AS count FROM message_send_answers",
                    ] {
                        counts.push(
                            sqlx::query::<ProjectionDatabase>(statement)
                                .fetch_one(scope.executor())
                                .await?
                                .try_get("count")?,
                        );
                    }
                    Ok(counts)
                })
            })
            .await
            .unwrap()
    }
    pub async fn local_outcome(&self, item: ItemId) -> Option<RecipientOutcome<CarrierAcceptance>> {
        let config = self.host.config;
        let binding = self.host.binding.clone();
        self.store
            .transaction(move |scope| {
                Box::pin(async move {
                    Inbox::<_, RouterDeliveryTypes, _>::new(
                        scope,
                        config.local.owner,
                        config.local.limits,
                        binding.as_ref(),
                    )
                    .local_outcome(item)
                    .await
                    .map_err(StoreError::backend)
                })
            })
            .await
            .unwrap()
    }
}
