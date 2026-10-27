//! The existing reader cadence prompts the native custody owner; it never delivers a second copy.
use super::{BoardError, PRESENCE_INTERVAL, ReaderDeliveryOwner, TargetPresence};
#[cfg(test)]
use crate::subscription_delivery::subscription_service::OwnerObservation;

impl ReaderDeliveryOwner {
    pub(super) async fn resume_native_held_mail(
        &mut self,
        target: &collaboration_protocol::SessionRef,
    ) -> Result<bool, BoardError> {
        let Some(runtime) = self.native_mail.get().and_then(std::sync::Weak::upgrade) else {
            self.native_checked_presence.clear();
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
        self.native_checked_presence
            .retain(|id, _| held_ids.contains(id));
        let mut running = false;
        for item in &items {
            if self
                .native_checked_presence
                .get(item)
                .is_some_and(|checked| {
                    self.clock.monotonic_now().duration_since(*checked) < PRESENCE_INTERVAL
                })
            {
                continue;
            }
            self.native_checked_presence
                .insert(item.clone(), self.clock.monotonic_now());
            let is_running = matches!(
                self.presence.presence(target).await,
                Ok(TargetPresence::Running)
            );
            #[cfg(test)]
            self.observe(OwnerObservation::NativeHeldChecked {
                item: item.clone(),
                running: is_running,
            });
            if is_running {
                // Presence belongs to this reader, so one positive check also
                // releases its items whose checks were staggered on earlier passes.
                running = true;
                break;
            }
        }
        if running {
            let ready = items
                .iter()
                .map(|item| {
                    self.native_checked_presence
                        .insert(item.clone(), self.clock.monotonic_now());
                    item.clone()
                })
                .collect();
            runtime.release_held_mail(ready).await.map_err(|error| {
                tracing::warn!(%error, "native held-mail release failed");
                BoardError::board_unavailable()
            })?;
        }
        Ok(!items.is_empty())
    }
}
