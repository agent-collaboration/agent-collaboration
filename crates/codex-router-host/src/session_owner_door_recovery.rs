//! Settle failed-owner callers and start the registry's successor.

use super::{SessionOwnerAddress, SessionOwnerDoor};
use crate::{
    session_owner_execution::SessionOwnerExecution, session_owner_registry::FailedOwnerSettlement,
    session_store_lane::SessionStoreSettlement,
};
use acp_client_runtime::{acp_transport::SlotEvidence, session_owner_task::SessionOwnerTask};
use collaboration_protocol::SessionRef;
use provider_session_machines::session_owner::{RemoteEffect, RevokeReason, SessionOwnerReply};
use std::{collections::BTreeMap, sync::Arc};

impl SessionOwnerDoor {
    pub(super) fn run_registered(
        self: &Arc<Self>,
        target: SessionRef,
        mut task: SessionOwnerTask,
        mut ports: SessionOwnerExecution,
        mut owner: Arc<SessionOwnerAddress>,
        work_roots: crate::provider_work_roots::ProviderWorkRoots,
    ) {
        let weak = Arc::downgrade(self);
        let stop = self.shutdown.clone();
        self.tasks.spawn(async move {
            loop {
                match task.run(ports, stop.clone()).await {
                    Ok(()) => {
                        owner.calls.stop();
                        return;
                    }
                    Err(error) => tracing::debug!(%error, "provider session owner failed"),
                }
                let Some((next_task, next_ports, next_owner)) = weak
                    .upgrade()
                    .and_then(|door| door.recover_session(&target, &owner, work_roots.clone()))
                else {
                    owner.calls.stop();
                    return;
                };
                task = next_task;
                ports = next_ports;
                owner = next_owner;
            }
        });
    }

    pub(super) fn recover_session(
        &self,
        target: &SessionRef,
        previous: &Arc<SessionOwnerAddress>,
        work_roots: crate::provider_work_roots::ProviderWorkRoots,
    ) -> Option<(
        SessionOwnerTask,
        SessionOwnerExecution,
        Arc<SessionOwnerAddress>,
    )> {
        if self.shutdown.is_cancelled() {
            return None;
        }
        let failed = previous.owner.address.incarnation;
        let FailedOwnerSettlement {
            replacement,
            handover,
            slot_evidence,
            ..
        } = self
            .registry
            .replace_failed_with(target, failed, self.connection, |incarnation| {
                self.wire.abandon_incarnation(incarnation)
            })
            .ok()?;
        let registered = self
            .registry
            .session(target)
            .filter(|session| session.owner.address == replacement.owner.address)?;
        let (successor, ports) = self
            .prepare_adopted(target.clone(), registered, None, Some(failed), work_roots)
            .ok()?;
        self.sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(target.clone(), successor.clone());

        let callers = previous.calls.clone();
        #[cfg(test)]
        let pause = self.preparation_fixture.clone();
        self.tasks.spawn(async move {
            let mut results = BTreeMap::new();
            for (marked, ticket) in handover.cleanups {
                let saved = matches!(
                    ticket.wait().await,
                    Ok(SessionStoreSettlement::Operation(_)
                        | SessionStoreSettlement::MarkNeverWritten)
                );
                results.insert(marked.input, saved);
            }
            #[cfg(test)]
            if let Some(release) = pause.hold_recovery() {
                let _test_withdrew_pause = release.await;
            }
            for input in callers.outstanding_inputs() {
                let taken = slot_evidence.iter().any(|(authorization, effect)| {
                    authorization.input_id == input.input && *effect == SlotEvidence::Taken
                });
                let saved = results.remove(&input.input).unwrap_or(false);
                let reply = if taken {
                    SessionOwnerReply::InputFinished {
                        input,
                        remote: RemoteEffect::Unknown,
                    }
                } else {
                    SessionOwnerReply::InputUnsent {
                        input,
                        reason: RevokeReason::ConnectionLost,
                    }
                };
                callers.settle(reply, saved);
            }
            callers.stop();
        });
        Some((replacement.task, ports, successor))
    }
}

impl Drop for SessionOwnerDoor {
    fn drop(&mut self) {
        self.shutdown.cancel();
    }
}
