//! Start and address a session-owner task through the Host registry.

use crate::{
    session_owner_calls::{
        CancelReceipt, CancelTarget, InputAcceptance, OwnedSettlement, SessionOwnerCalls,
    },
    session_owner_execution::SessionOwnerExecution,
    session_owner_publication::SessionOwnerPublisher,
    session_owner_registry::{RegisteredOwner, SessionOwnerRegistry},
    session_owner_storage::SessionOwnerStorage,
};
use acp_client_runtime::{
    ProviderPromptContent, owner_wire::OwnerAcpWire, session_owner_task::OwnerMailboxUnavailable,
};
use collaboration_protocol::SessionRef;
use collaboration_runtime::{ProviderOperationStore, ProviderSessionEventHub};
use provider_session_machines::{
    session_lifecycle_machine::{LoadAttempt, RestoreVia},
    session_owner::{
        ConnectionEpoch, OwnerEvent, OwnerInput, PromptAdmission, SessionOwnerCommand,
        SessionOwnerRejection, SessionSnapshot,
    },
};
use router_clock::RuntimeClock;
use std::{
    collections::BTreeMap,
    num::NonZeroUsize,
    sync::{Arc, Mutex},
};
use tokio::sync::oneshot;
use tokio::sync::{Mutex as AsyncMutex, watch};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum OwnerDoorError {
    #[error(transparent)]
    Mailbox(#[from] OwnerMailboxUnavailable),
    #[error("the Router couldn't prepare this session's owner")]
    Unavailable,
    #[error("the session owner refused the command: {rejection:?}")]
    Refused {
        rejection: SessionOwnerRejection,
        snapshot: Option<Box<SessionSnapshot>>,
    },
    #[error("the session owner stopped before giving a result")]
    OwnerStopped,
}

pub struct SessionOwnerDoor {
    registry: Arc<SessionOwnerRegistry>,
    clock: Arc<dyn RuntimeClock>,
    hub: Arc<ProviderSessionEventHub>,
    wire: Arc<OwnerAcpWire>,
    approvals:
        acp_client_runtime::OwnerApprovalBindings<crate::acp_interaction_port::HostInteractionPort>,
    connection: ConnectionEpoch,
    retired: CancellationToken,
    shutdown: CancellationToken,
    tasks: TaskTracker,
    sessions: Mutex<BTreeMap<SessionRef, Arc<SessionOwnerAddress>>>,
    snapshot_observer: Arc<Mutex<Option<Arc<dyn SessionOwnerSnapshotObserver>>>>,
    #[cfg(test)]
    preparation_fixture: Arc<crate::session_owner_preparation_fixture::PreparationFixture>,
}

pub(crate) trait SessionOwnerSnapshotObserver: Send + Sync + 'static {
    fn observe(&self, session: &str, previous: &SessionSnapshot, current: &SessionSnapshot);
}

pub struct SessionOwnerAddress {
    owner: RegisteredOwner,
    calls: Arc<SessionOwnerCalls>,
    storage: SessionOwnerStorage,
    wire: Arc<OwnerAcpWire>,
    #[cfg(test)]
    preparation_fixture: Arc<crate::session_owner_preparation_fixture::PreparationFixture>,
}

pub(crate) struct OwnerDeliveryHandle {
    pub(crate) accepted: oneshot::Receiver<InputAcceptance>,
    pub(crate) terminal: oneshot::Receiver<OwnedSettlement>,
}

impl SessionOwnerDoor {
    pub(crate) fn new(
        wire: Arc<OwnerAcpWire>,
        approvals: acp_client_runtime::OwnerApprovalBindings<
            crate::acp_interaction_port::HostInteractionPort,
        >,
        clock: Arc<dyn RuntimeClock>,
        store: Arc<AsyncMutex<ProviderOperationStore>>,
        hub: Arc<ProviderSessionEventHub>,
        retired: CancellationToken,
        #[cfg(test)] preparation_fixture: Arc<
            crate::session_owner_preparation_fixture::PreparationFixture,
        >,
    ) -> Self {
        let mailbox = NonZeroUsize::new(32).unwrap_or(NonZeroUsize::MIN);
        Self {
            registry: Arc::new(SessionOwnerRegistry::new(
                store,
                clock.clone(),
                mailbox,
                mailbox,
            )),
            clock,
            hub,
            wire,
            approvals,
            connection: ConnectionEpoch(Uuid::now_v7()),
            retired,
            shutdown: CancellationToken::new(),
            tasks: TaskTracker::new(),
            sessions: Mutex::default(),
            snapshot_observer: Arc::default(),
            #[cfg(test)]
            preparation_fixture,
        }
    }

    pub(crate) fn install_binding_permit(
        &self,
        binding: Arc<crate::binding_dispatch_permit::BindingUncertainty>,
    ) {
        self.registry.install_binding_permit(binding);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) async fn wait_for_accepted_mark_fixture(
        &self,
        target: &SessionRef,
        operation: &collaboration_protocol::OperationId,
    ) -> Result<(), OwnerDoorError> {
        let session = self
            .registry
            .session(target)
            .ok_or(OwnerDoorError::Unavailable)?;
        session.lane.wait_for_accepted_mark(operation).await;
        Ok(())
    }

    pub(crate) fn open(
        self: &Arc<Self>,
        target: SessionRef,
        work_roots: crate::provider_work_roots::ProviderWorkRoots,
    ) -> Result<Arc<SessionOwnerAddress>, OwnerDoorError> {
        if self.retired.is_cancelled() {
            return Err(OwnerDoorError::Unavailable);
        }
        let mut sessions = self
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(owner) = sessions.get(&target) {
            return Ok(Arc::clone(owner));
        }
        let registered = self.registry.get_or_insert(target.clone(), self.connection);
        let Some((launch, lane)) = registered.launch else {
            return Err(OwnerDoorError::Unavailable);
        };
        let (session, ports) = self.prepare_adopted(
            target.clone(),
            registered.session,
            Some(lane),
            None,
            work_roots.clone(),
        )?;
        self.run_registered(
            target.clone(),
            launch.task,
            ports,
            session.clone(),
            work_roots,
        );
        sessions.insert(target, session.clone());
        Ok(session)
    }

    fn prepare_adopted(
        &self,
        target: SessionRef,
        registered: crate::session_owner_registry::RegisteredSession,
        lane: Option<crate::session_store_lane::SessionStoreLaneTask>,
        replaced: Option<provider_session_machines::session_owner::OwnerIncarnation>,
        work_roots: crate::provider_work_roots::ProviderWorkRoots,
    ) -> Result<(Arc<SessionOwnerAddress>, SessionOwnerExecution), OwnerDoorError> {
        let session_id = String::from(target.session_id.clone());
        let board = collaboration_protocol::ProviderIdentity::session_to_board(&target)
            .map_err(|_| OwnerDoorError::Unavailable)?;
        let (inbox, publisher_task) = self
            .hub
            .owner_inbox(board, NonZeroUsize::new(16).unwrap_or(NonZeroUsize::MIN));
        inbox
            .install_incarnation(
                replaced.map(|old| old.0),
                registered.owner.address.incarnation.0,
            )
            .map_err(|_| OwnerDoorError::Unavailable)?;
        if let Some(publisher) = publisher_task {
            self.tasks.spawn(publisher.run(self.shutdown.clone()));
        }
        if let Some(lane) = lane {
            self.tasks
                .spawn(lane.run_tracked(self.shutdown.clone(), self.tasks.clone()));
        }
        let calls = Arc::new(SessionOwnerCalls::default());
        let registered_owner = registered.owner;
        let storage = SessionOwnerStorage::new(registered.lane, self.tasks.clone());
        let ports = SessionOwnerExecution::new(
            target.clone(),
            self.wire.clone(),
            self.approvals.clone(),
            storage.clone(),
            SessionOwnerPublisher::new(inbox, self.clock.clone(), self.tasks.clone()),
            calls.clone(),
            work_roots,
            #[cfg(test)]
            self.preparation_fixture.clone(),
        );
        let retired = self.retired.clone();
        let stop = self.shutdown.clone();
        let completion = registered_owner.handle.completion_port();
        let address = registered_owner.address;
        self.tasks.spawn(async move {
            tokio::select! {
                () = stop.cancelled() => {}
                () = retired.cancelled() => {
                    let _owner_gone = completion.send_stream(address, OwnerEvent::ConnectionLost, &stop).await;
                }
            }
        });
        let mut snapshots = registered_owner.handle.snapshots();
        let mut previous = snapshots.borrow_and_update().clone();
        let observer = Arc::clone(&self.snapshot_observer);
        let registry = Arc::clone(&self.registry);
        let stopped = self.shutdown.clone();
        self.tasks.spawn(async move {
            loop {
                tokio::select! {
                    () = stopped.cancelled() => return,
                    changed = snapshots.changed() => {
                        if changed.is_err() { return; }
                    }
                }
                let next = snapshots.borrow_and_update().clone();
                if !registry
                    .session(&target)
                    .is_some_and(|session| session.owner.address == next.owner)
                {
                    return;
                }
                let selected = observer
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone();
                if let Some(selected) = selected {
                    selected.observe(&session_id, &previous, &next);
                }
                previous = next;
            }
        });
        let session = Arc::new(SessionOwnerAddress {
            owner: registered_owner,
            calls,
            storage,
            wire: self.wire.clone(),
            #[cfg(test)]
            preparation_fixture: self.preparation_fixture.clone(),
        });
        Ok((session, ports))
    }
    pub async fn shutdown(&self) {
        if !self.shutdown.is_cancelled() {
            self.retired.cancel();
            let owners: Vec<_> = self
                .sessions
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .values()
                .map(|session| Arc::clone(&session.calls))
                .collect();
            let replies =
                futures_util::future::join_all(owners.iter().map(|calls| calls.wait_for_replies()));
            let _drained = router_clock::timeout(
                self.clock.as_ref(),
                std::time::Duration::from_secs(10),
                replies,
            )
            .await;
        }
        self.shutdown.cancel();
        self.tasks.close();
        self.tasks.wait().await;
    }

    pub(crate) async fn cancel_waiting_inputs(&self) {
        let owners: Vec<_> = self
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .values()
            .cloned()
            .collect();
        for owner in owners {
            let queue = owner.snapshots().borrow().queue.clone();
            for entry in queue {
                if !entry.started {
                    let _settled = owner.cancel_queued(entry.input.input).await;
                }
            }
        }
    }

    pub(crate) fn existing(&self, target: &SessionRef) -> Option<Arc<SessionOwnerAddress>> {
        self.sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(target)
            .cloned()
    }

    pub(crate) fn existing_provider_session(
        &self,
        provider_id: &str,
    ) -> Option<Arc<SessionOwnerAddress>> {
        self.sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .find(|(target, _)| String::from(target.session_id.clone()) == provider_id)
            .map(|(_, owner)| owner.clone())
    }

    pub(crate) fn install_snapshot_observer(
        &self,
        observer: Arc<dyn SessionOwnerSnapshotObserver>,
    ) {
        *self
            .snapshot_observer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(observer);
    }
}

impl SessionOwnerAddress {
    pub(crate) async fn cancel_turn(
        &self,
        generation: provider_session_machines::SessionGeneration,
        turn: provider_session_machines::session_turn_machine::TurnId,
    ) -> Result<CancelReceipt, OwnerDoorError> {
        self.cancel(
            CancelTarget::Turn(generation, turn),
            SessionOwnerCommand::CancelTurn { generation, turn },
        )
        .await
    }

    pub(crate) async fn cancel_operation(
        &self,
        operation: Uuid,
    ) -> Result<CancelReceipt, OwnerDoorError> {
        self.cancel(
            CancelTarget::Operation(operation),
            SessionOwnerCommand::CancelOperation { target: operation },
        )
        .await
    }

    async fn cancel(
        &self,
        target: CancelTarget,
        command: SessionOwnerCommand,
    ) -> Result<CancelReceipt, OwnerDoorError> {
        let receipt = self
            .calls
            .expect_cancel(target)
            .ok_or(OwnerDoorError::Mailbox(OwnerMailboxUnavailable::Busy))?;
        if let Err(error) = self.admit(command).await {
            self.calls.forget_cancel(target);
            return Err(error);
        }
        receipt.await.map_err(|_| OwnerDoorError::OwnerStopped)
    }
    pub(crate) fn report_event(
        &self,
        event: OwnerEvent,
    ) -> Result<(), acp_client_runtime::EventSinkClosed> {
        let completion = self
            .owner
            .handle
            .completion_port()
            .reserve(self.owner.address)
            .map_err(|_| acp_client_runtime::EventSinkClosed)?;
        completion.send(event);
        Ok(())
    }
    #[must_use]
    pub fn snapshots(&self) -> watch::Receiver<SessionSnapshot> {
        self.owner.handle.snapshots()
    }

    pub(crate) fn catalog(&self) -> Option<provider_session_machines::ProviderSettingsCatalog> {
        let snapshot = self.snapshots();
        let provider_session_machines::session_lifecycle_machine::SessionLifecycle::Live {
            generation,
        } = snapshot.borrow().lifecycle
        else {
            return None;
        };
        self.wire
            .session_catalog(acp_client_runtime::acp_transport::TransportRun {
                incarnation: self.owner.address.incarnation,
                generation,
            })
    }

    pub(crate) async fn cancel_queued(
        &self,
        input: session_event_model::InputId,
    ) -> Result<(), OwnerDoorError> {
        let generation = match self.snapshots().borrow().lifecycle {
            provider_session_machines::session_lifecycle_machine::SessionLifecycle::Live {
                generation,
            }
            | provider_session_machines::session_lifecycle_machine::SessionLifecycle::Loading {
                generation,
                ..
            } => generation,
            _ => return Err(OwnerDoorError::Unavailable),
        };
        self.admit(SessionOwnerCommand::CancelQueued { generation, input })
            .await
    }

    pub async fn restore(
        &self,
        operation: Uuid,
        via: RestoreVia,
    ) -> Result<OwnedSettlement, OwnerDoorError> {
        self.settle(
            operation,
            SessionOwnerCommand::Restore {
                attempt: LoadAttempt(operation),
                via,
            },
        )
        .await
    }

    pub(crate) async fn restore_explicit(
        &self,
        operation: Uuid,
        via: RestoreVia,
        record: Box<collaboration_runtime::ProviderSessionRecord>,
    ) -> Result<OwnedSettlement, OwnerDoorError> {
        let attempt = LoadAttempt(operation);
        self.storage.expect_explicit_restore(attempt, record);
        let result = self.restore(operation, via).await;
        self.storage.forget_restore(attempt);
        result
    }

    pub(crate) async fn restore_for_delivery(
        &self,
        operation: Uuid,
    ) -> Result<OwnedSettlement, OwnerDoorError> {
        let attempt = LoadAttempt(operation);
        self.storage.expect_delivery_restore(attempt);
        let result = self.restore(operation, RestoreVia::Load).await;
        self.storage.forget_restore(attempt);
        result
    }

    pub(crate) async fn join_restore(
        &self,
        operation: Uuid,
        run: provider_session_machines::session_owner::RunAttempt<LoadAttempt>,
    ) -> Result<provider_session_machines::session_owner::RestoreResult, OwnerDoorError> {
        let reply = self.calls.expect_restore(operation);
        if let Err(error) = self
            .admit(SessionOwnerCommand::DependOnRestore { operation, run })
            .await
        {
            self.calls.release_operation(operation);
            return self
                .snapshots()
                .borrow()
                .last_restore
                .clone()
                .filter(|result| result.attempt == run)
                .ok_or(error);
        }
        reply.await.map_err(|_| OwnerDoorError::OwnerStopped)
    }

    pub async fn submit_input(
        &self,
        input: OwnerInput,
        content: ProviderPromptContent,
        admission: PromptAdmission,
    ) -> Result<OwnedSettlement, OwnerDoorError> {
        let command = prompt_with_requirements(input.clone(), admission, &content);
        self.calls.insert_content(&input, content);
        let result = self.settle(input.operation, command).await;
        if result.is_err() {
            self.calls.release_input(&input.input);
        }
        result
    }

    pub(crate) fn remember_approval(
        &self,
        input: session_event_model::InputId,
        context: crate::ExternalProviderApprovalContext,
    ) {
        self.calls.remember_approval(input, context);
    }

    pub(crate) async fn start_delivery(
        &self,
        input: OwnerInput,
        content: ProviderPromptContent,
        command: SessionOwnerCommand,
    ) -> Result<OwnerDeliveryHandle, OwnerDoorError> {
        self.start_delivery_with_evidence(input, content, command, None, None)
            .await
    }

    pub(crate) async fn start_delivery_with_evidence(
        &self,
        input: OwnerInput,
        content: ProviderPromptContent,
        command: SessionOwnerCommand,
        evidence: Option<crate::session_owner_delivery_evidence::OwnerDeliveryEvidence>,
        cold_restore: Option<LoadAttempt>,
    ) -> Result<OwnerDeliveryHandle, OwnerDoorError> {
        use provider_session_machines::session_owner::DeliveryInput;

        let command = match command {
            SessionOwnerCommand::Prompt { input, admission } => {
                prompt_with_requirements(input, admission, &content)
            }
            SessionOwnerCommand::Steer { .. } | SessionOwnerCommand::SteerExisting { .. }
                if !content.selected_skills().is_empty() =>
            {
                self.calls.release_input(&input.input);
                return Err(OwnerDoorError::Refused {
                    rejection: SessionOwnerRejection::Refused(
                        provider_session_machines::session_owner::SessionOwnerRefusal::SkillSelectionChanged,
                    ),
                    snapshot: None,
                });
            }
            command => command,
        };
        let command = if let Some(attempt) = cold_restore {
            let record_evidence = evidence.is_some();
            if let Some(evidence) = evidence {
                self.calls
                    .expect_delivery_evidence(input.operation, evidence);
            }
            self.storage.expect_delivery_restore(attempt);
            SessionOwnerCommand::QueueColdDelivery {
                input: input.clone(),
                attempt,
                selected: content.selected_skills().to_vec(),
                record_evidence,
            }
        } else if let Some(evidence) = evidence {
            let delivery = match command {
                SessionOwnerCommand::Prompt { admission, .. } => DeliveryInput::Prompt {
                    admission,
                    selected: Vec::new(),
                },
                SessionOwnerCommand::PromptWithSkills {
                    admission,
                    selected,
                    ..
                } => DeliveryInput::Prompt {
                    admission,
                    selected,
                },
                SessionOwnerCommand::Steer { supported, .. } => DeliveryInput::Steer { supported },
                _ => return Err(OwnerDoorError::Unavailable),
            };
            self.calls
                .expect_delivery_evidence(input.operation, evidence);
            SessionOwnerCommand::PlaceDelivery {
                input: input.clone(),
                delivery,
            }
        } else {
            command
        };
        self.calls.insert_content(&input, content);
        let terminal = self.calls.expect_settlement(input.operation);
        let accepted = self.calls.expect_acceptance(input.operation);
        let admission = self.admit(command).await;
        #[cfg(test)]
        let (admission, ack_finished) = match self.preparation_fixture.take_delivery_ack_pause() {
            Some(pause) => pause.resolve(admission).await,
            None => (admission, None),
        };
        let result = match admission {
            // The failed-owner recovery still owns these records. Its writer
            // evidence decides what reached the provider even if the task
            // stopped before sending the admission acknowledgement.
            Ok(()) | Err(OwnerDoorError::OwnerStopped) => {
                Ok(OwnerDeliveryHandle { accepted, terminal })
            }
            Err(error) => {
                if let Some(attempt) = cold_restore {
                    self.storage.forget_restore(attempt);
                }
                self.calls.release_input(&input.input);
                self.calls.release_operation(input.operation);
                Err(error)
            }
        };
        #[cfg(test)]
        if let Some(finished) = ack_finished {
            let _test_withdrew_probe = finished.send(());
        }
        result
    }

    pub(crate) fn defer_queued_admission(
        &self,
        admission: collaboration_runtime::ProviderOperationAdmission,
    ) -> Result<(), crate::session_store_lane::SessionStoreOfferError> {
        self.storage
            .defer_queued_admission(self.owner.address.incarnation, admission)
    }

    pub(crate) fn release_queued_admission(&self, operation: &collaboration_protocol::OperationId) {
        self.storage
            .release_queued_admission(self.owner.address.incarnation, operation);
    }

    pub(crate) fn expect_confirmed_mode(
        &self,
        setting: provider_session_machines::session_owner::OwnerSetting,
        asked: String,
    ) {
        self.storage.expect_confirmed_mode(setting, asked);
    }

    pub async fn settle(
        &self,
        operation: Uuid,
        command: SessionOwnerCommand,
    ) -> Result<OwnedSettlement, OwnerDoorError> {
        let answer = self.calls.expect_settlement(operation);
        let result = self.admit(command).await;
        if let Err(error) = result {
            self.calls.release_operation(operation);
            return Err(error);
        }
        answer.await.map_err(|_| OwnerDoorError::OwnerStopped)
    }

    pub async fn admit(&self, command: SessionOwnerCommand) -> Result<(), OwnerDoorError> {
        let accepted = self.owner.handle.try_submit(self.owner.address, command)?;
        accepted
            .await
            .map_err(|_| OwnerDoorError::OwnerStopped)?
            .map_err(|refusal| OwnerDoorError::Refused {
                rejection: refusal.rejection,
                snapshot: Some(refusal.snapshot),
            })
    }

    #[cfg(test)]
    pub(crate) fn hold_completion_capacity(
        &self,
    ) -> Vec<acp_client_runtime::session_owner_task::OwnerCompletion> {
        let port = self.owner.handle.completion_port();
        std::iter::from_fn(|| port.reserve(self.owner.address).ok()).collect()
    }

    #[cfg(test)]
    pub(crate) async fn wait_for_recovery_replies(&self) {
        self.calls.wait_for_replies().await;
    }

    #[cfg(test)]
    pub(crate) fn hold_provider_writer_capacity(&self) -> impl Sized + use<> {
        self.wire.hold_writer_capacity_for_test()
    }
}

fn prompt_with_requirements(
    input: OwnerInput,
    admission: PromptAdmission,
    content: &ProviderPromptContent,
) -> SessionOwnerCommand {
    if content.selected_skills().is_empty() {
        SessionOwnerCommand::Prompt { input, admission }
    } else {
        SessionOwnerCommand::PromptWithSkills {
            input,
            admission,
            selected: content.selected_skills().to_vec(),
        }
    }
}

#[cfg(test)]
#[path = "session_owner_door_test.rs"]
mod tests;

#[path = "session_owner_door_creation.rs"]
mod creation;

#[path = "session_owner_door_recovery.rs"]
mod recovery;
