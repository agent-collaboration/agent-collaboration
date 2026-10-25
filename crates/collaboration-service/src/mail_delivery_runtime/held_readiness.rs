//! Reader readiness releases only the library's retained native custody.
use super::*;

impl MailDeliveryRuntime {
    pub(crate) fn attach_reader_readiness(self: &Arc<Self>) -> Result<(), String> {
        self.reader_actors.attach_native_mail(Arc::downgrade(self))
    }

    pub(crate) async fn held_mail_ids_for_target(
        &self,
        target: &collaboration_protocol::SessionRef,
    ) -> Result<Vec<collaboration_protocol::PushId>, StoreError> {
        self.owner
            .held_mail_push_ids(target)
            .await
            .map_err(StoreError::backend)
    }

    pub(crate) async fn release_held_mail(
        &self,
        items: Vec<collaboration_protocol::PushId>,
    ) -> Result<(), StoreError> {
        let config = self.config;
        let now = SyncTime::from_unix_millis(self.clock.now().timestamp_millis());
        let admitted = self
            .store
            .transaction(move |scope| {
                Box::pin(async move {
                    let mut admitted = false;
                    for item in items {
                        // Recheck the current projection, not the reader's earlier
                        // selection. A preceding release may already have made it Pending.
                        if !interaction_storage::mark_released_mail_pending_in_scope(scope, &item)
                            .await
                            .map_err(StoreError::backend)?
                        {
                            continue;
                        }
                        let item = ItemId::new(
                            uuid::Uuid::parse_str(item.as_str()).map_err(StoreError::backend)?,
                        )
                        .map_err(StoreError::backend)?;
                        let released = Inbox::<_, RouterDeliveryTypes>::new(
                            scope,
                            config.owner,
                            config.limits,
                        )
                        .release_held::<LocalMailTask>(item, now)
                        .await
                        .map_err(StoreError::backend)?
                        .is_some();
                        if !released {
                            // Roll back both the projection and any SDK update.
                            return Err(StoreError::backend(
                                "readiness hold has no releasable SDK custody",
                            ));
                        }
                        admitted = true;
                    }
                    Ok(admitted)
                })
            })
            .await?;
        if admitted {
            self.host.wake_at(now).await.map_err(StoreError::backend)?;
        }
        Ok(())
    }
}
