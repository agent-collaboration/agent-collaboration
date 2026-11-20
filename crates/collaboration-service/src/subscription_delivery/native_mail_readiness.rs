//! Volatile reader prompts bridge native outcomes to the existing readiness loop.
use super::{
    BoardError, ReaderDeliveryCommand, ServiceHandleLifetime, SessionRef,
    SubscriptionDeliveryService, reader_for_target,
};

impl SubscriptionDeliveryService {
    pub(crate) fn attach_native_mail(
        &self,
        runtime: std::sync::Weak<crate::MailDeliveryRuntime>,
    ) -> Result<(), String> {
        if let Err(runtime) = self.inner.native_mail.set(runtime)
            && !self
                .inner
                .native_mail
                .get()
                .is_some_and(|existing| existing.ptr_eq(&runtime))
        {
            return Err("reader readiness already has a different native mail runtime".into());
        }
        Ok(())
    }

    pub(crate) fn prompt_native_mail_outcome(&self, target: SessionRef) {
        if self.inner.shutdown.is_cancelled() || self.inner.native_mail.get().is_none() {
            return;
        }
        let service = Self {
            inner: self.inner.clone(),
            lifetime: ServiceHandleLifetime::Background,
        };
        // Outcome hooks may call this before commit. The reader's native read waits
        // for that transaction and rechecks custody; a rolled-back hint has no effect.
        self.inner.tasks.spawn(async move {
            let result = service.observe_native_mail_change(&target).await;
            if let Err(error) = result && !service.inner.shutdown.is_cancelled() {
                tracing::warn!(%error, "native mail-outcome reader prompt failed; startup will rediscover custody");
            }
        });
    }

    async fn observe_native_mail_change(&self, target: &SessionRef) -> Result<(), BoardError> {
        let reader = reader_for_target(target)?;
        loop {
            self.require_started().await?;
            let sender = self.ensure_owner(reader.clone()).await?;
            let (observed, response) = tokio::sync::oneshot::channel();
            if sender
                .send(ReaderDeliveryCommand::NativeMailChanged { observed })
                .await
                .is_ok()
                && response.await.is_ok()
            {
                return Ok(());
            }
        }
    }
}

#[cfg(test)]
#[path = "tests/reconcile_retry_proofs.rs"]
mod reconcile_retry_proofs;
