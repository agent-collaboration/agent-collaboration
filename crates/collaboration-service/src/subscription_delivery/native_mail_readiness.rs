//! Volatile reader prompts bridge native outcomes to the existing readiness loop.
use super::{
    BoardError, ReaderReconcilePurpose, ReaderScanScope, ServiceHandleLifetime, SessionRef,
    SubscriptionDeliveryService, reader_for_target,
};

impl SubscriptionDeliveryService {
    pub(crate) fn prompt_project_mail_outcome(&self, item: fleet_sync_protocol::ItemId) {
        if self.inner.shutdown.is_cancelled() {
            return;
        }
        let mail = self.inner.native_mail.get().cloned();
        let port = self
            .inner
            .follow_admission
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let Some((mail, publisher)) = mail
            .and_then(|mail| mail.upgrade())
            .zip(port.and_then(|port| port.publisher.upgrade()))
        else {
            return;
        };
        self.inner.tasks.spawn(async move {
            let result = async {
                mail.await_native_outcome_commit(item)
                    .await
                    .map_err(|_| BoardError::board_unavailable())?;
                publisher.prompt_terminal_item(item).await
            }
            .await;
            if let Err(error) = result {
                tracing::warn!(%error, "committed project mail frontier hint delayed");
            }
        });
    }
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
            let result = async {
                let reader = reader_for_target(&target)?;
                service.require_started().await?;
                service.reconcile_owned_scope(reader, ReaderScanScope::All, ReaderReconcilePurpose::NativeMail).await
            }.await;
            if let Err(error) = result && !service.inner.shutdown.is_cancelled() {
                tracing::warn!(%error, "native mail-outcome reader prompt failed; startup will rediscover custody");
            }
        });
    }
}
