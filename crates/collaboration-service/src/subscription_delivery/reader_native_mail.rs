//! The existing reader cadence prompts the native custody owner; it never delivers a second copy.
//!
//! A held item's target is checked once when the reader first sees the item, and again
//! only after a signal that it may have loaded; there is no presence timer.
use super::{BoardError, ReaderDeliveryOwner, TargetPresence};
#[cfg(test)]
use crate::subscription_delivery::subscription_service::OwnerObservation;

impl ReaderDeliveryOwner {
    pub(super) async fn resume_native_held_mail(
        &mut self,
        target: &collaboration_protocol::SessionRef,
    ) -> Result<bool, BoardError> {
        let Some(runtime) = self.native_mail.get().and_then(std::sync::Weak::upgrade) else {
            self.native_checked.clear();
            return Ok(false);
        };
        let items = runtime
            .held_mail_ids_for_target(target)
            .await
            .map_err(|error| {
                tracing::warn!(%error, "native held-mail discovery failed");
                BoardError::board_unavailable()
            })?;
        let held_ids = items.iter().collect::<std::collections::HashSet<_>>();
        self.native_checked.retain(|id| held_ids.contains(id));
        let Some(item) = items
            .iter()
            .find(|item| !self.native_checked.contains(*item))
        else {
            return Ok(!items.is_empty());
        };
        // Presence belongs to this reader: one check answers for all its held items.
        self.held_mail.record_position().await;
        let running = matches!(
            self.presence.presence(target).await,
            Ok(TargetPresence::Running)
        );
        #[cfg(test)]
        self.observe(OwnerObservation::NativeHeldChecked {
            item: item.clone(),
            running,
        });
        #[cfg(not(test))]
        let _ = item;
        self.native_checked.extend(items.iter().cloned());
        if running {
            runtime
                .release_held_mail(items.clone())
                .await
                .map_err(|error| {
                    tracing::warn!(%error, "native held-mail release failed");
                    BoardError::board_unavailable()
                })?;
        }
        Ok(!items.is_empty())
    }
}
