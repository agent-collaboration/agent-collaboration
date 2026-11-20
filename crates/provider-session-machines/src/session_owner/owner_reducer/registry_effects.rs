use super::publication_effects::WIRE_BUDGET;
use super::{OwnerRejection, OwnerResult, OwnerTransition};
use crate::session_cell_machine::SessionCellEvent;
use crate::session_lifecycle_machine::{LoadAttempt, SessionLifecycle};
use crate::session_owner::{
    CreateOutcome, CreateProgress, OwnerDeadline, OwnerEffect, RemoteEffect, RestoreFailure,
    RestoreResult, RunAttempt, SessionOwnerEffect, SessionOwnerRefusal, SessionPublication,
    SessionWireRequest,
};
use uuid::Uuid;

impl OwnerTransition {
    pub(super) fn start_create(&mut self, operation: Uuid) -> OwnerResult {
        if !matches!(self.next.lifecycle(), SessionLifecycle::NotLoaded { .. })
            || !self.next.ports.connection_alive
            || self.next.ports.create.is_some()
        {
            return Err(SessionOwnerRefusal::EffectInProgress.into());
        }
        self.next.ports.create = Some(operation);
        self.effects
            .push(OwnerEffect::Wire(SessionWireRequest::Create { operation }));
        self.arm(OwnerDeadline::Create(operation), WIRE_BUDGET);
        Ok(())
    }

    pub(super) fn create_progress(&mut self, progress: CreateProgress) -> OwnerResult {
        let operation = match &progress {
            CreateProgress::ProviderId { operation, .. }
            | CreateProgress::Adopted { operation }
            | CreateProgress::AlreadyOwned { operation }
            | CreateProgress::Failed { operation, .. } => *operation,
        };
        if self.next.ports.create != Some(operation) {
            return Err(OwnerRejection::IgnoredLate);
        }
        let outcome = match progress {
            CreateProgress::ProviderId { provider_id, .. } if !self.next.ports.adopting => {
                self.next.ports.adopting = true;
                self.effects.push(OwnerEffect::AdoptProviderId {
                    operation,
                    provider_id,
                });
                return Ok(());
            }
            CreateProgress::Adopted { .. } if self.next.ports.adopting => {
                self.cell_move(SessionCellEvent::CreateAdmitted)?;
                let Some(run) = self.next.cell.run_generation() else {
                    return Err(OwnerRejection::IgnoredLate);
                };
                self.release(SessionOwnerEffect::Publish(SessionPublication::Created(
                    run,
                )));
                CreateOutcome::Adopted(run)
            }
            CreateProgress::AlreadyOwned { .. } if self.next.ports.adopting => {
                CreateOutcome::SessionIdInUse
            }
            CreateProgress::Failed { remote, .. } => CreateOutcome::Failed(remote),
            _ => return Err(OwnerRejection::IgnoredLate),
        };
        self.next.ports.create = None;
        self.next.ports.adopting = false;
        self.effects
            .push(OwnerEffect::CreateSettled { operation, outcome });
        Ok(())
    }

    pub(super) fn expire_create(&mut self, operation: Uuid) -> OwnerResult {
        self.create_progress(CreateProgress::Failed {
            operation,
            remote: RemoteEffect::Unknown,
        })
    }

    pub(super) fn depend_on_restore(
        &mut self,
        operation: Uuid,
        run: RunAttempt<LoadAttempt>,
    ) -> OwnerResult {
        if !self
            .next
            .restore
            .is_some_and(|pending| pending.run() == run)
        {
            return Err(OwnerRejection::IgnoredLate);
        }
        self.next.ports.restore_dependents.insert(operation, run);
        Ok(())
    }

    pub(super) fn settle_restore(&mut self, result: RestoreResult) {
        self.record_terminal(crate::session_owner::SessionOwnerReply::RestoreFinished(
            result.clone(),
        ));
        self.next
            .ports
            .restore_dependents
            .retain(|&operation, run| {
                if *run == result.attempt {
                    self.effects.push(OwnerEffect::RestoreSettled {
                        operation,
                        result: result.clone(),
                    });
                    false
                } else {
                    true
                }
            });
        self.next.ports.last_restore = Some(result);
    }

    pub(super) fn connection_lost(&mut self) -> OwnerResult {
        if !self.next.ports.connection_alive {
            return Err(OwnerRejection::IgnoredLate);
        }
        self.next.ports.connection_alive = false;
        self.lose_transport_authorizations()?;
        if let Some(operation) = self.next.ports.create {
            self.expire_create(operation)?;
        }
        if let Some(pending) = self.next.restore {
            self.fail_restore(
                pending.run(),
                RestoreFailure::Disconnected,
                pending.remote(),
            )?;
        } else if let Some((run, _)) = self.closing() {
            self.close_deadline(run)?;
        } else if let SessionLifecycle::Live { generation } = self.next.lifecycle() {
            self.abandon_running_turn()?;
            let effects = self.cell_move(SessionCellEvent::ActorExited { generation })?;
            self.apply_entry_effects(effects)?;
            self.retire_generation(generation)?;
            self.release(SessionOwnerEffect::Publish(SessionPublication::Unloaded(
                generation,
            )));
        }
        self.next.ports.authorizations.clear();
        self.next.ports.restore_setting_fences.clear();
        Ok(())
    }
}
