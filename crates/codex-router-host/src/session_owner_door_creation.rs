//! Adopt the provider ID before releasing a provisional create.

use super::SessionOwnerDoor;
use crate::session_owner_execution::SessionOwnerExecution;
use acp_client_runtime::{
    ExternalProviderRuntimeError, RequestedProviderSettings,
    owner_session_creation::OwnerCreatedSetup,
    owner_session_open::{OwnerOpenedSession, OwnerSessionOpenSetup},
    session_owner_task::{OwnerCompletionPort, OwnerTaskError, SessionOwnerPorts},
};
use collaboration_protocol::{EndpointRef, OperationId, SessionId, SessionRef};
use provider_session_machines::session_owner::{
    CreateOutcome, CreateProgress, OwnerAddress, OwnerEffect, OwnerEvent, PublicationBatch,
    PublicationFact, RemoteEffect, SessionOwnerCommand, SessionPublication, SessionWireRequest,
};
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

type PreparedCreate = Result<CreatePreparation, ExternalProviderRuntimeError>;

enum CreatePreparation {
    Opened(OwnerOpenedSession, RequestedProviderSettings),
    Configured(OwnerOpenedSession, Box<OwnerCreatedSetup>),
}

struct ProvisionalCreate {
    door: std::sync::Weak<SessionOwnerDoor>,
    endpoint: EndpointRef,
    operation: OperationId,
    input: Option<(OwnerSessionOpenSetup, RequestedProviderSettings)>,
    work_roots: crate::provider_work_roots::ProviderWorkRoots,
    prepared: Arc<Mutex<Option<PreparedCreate>>>,
    rebase: Option<PublicationBatch>,
    settings: Option<OwnerCreatedSetup>,
    adopted: Option<SessionOwnerExecution>,
    response: Option<oneshot::Sender<Result<OwnerCreatedSetup, ExternalProviderRuntimeError>>>,
    stop: CancellationToken,
    configuring: Option<CancellationToken>,
}

impl SessionOwnerDoor {
    pub(crate) async fn create(
        self: &Arc<Self>,
        endpoint: EndpointRef,
        operation: OperationId,
        setup: OwnerSessionOpenSetup,
        requested: RequestedProviderSettings,
        work_roots: crate::provider_work_roots::ProviderWorkRoots,
    ) -> Result<OwnerCreatedSetup, ExternalProviderRuntimeError> {
        if self.retired.is_cancelled() {
            return Err(ExternalProviderRuntimeError::TransportFailure);
        }
        let correlation = Uuid::parse_str(operation.as_str()).map_err(|_| {
            ExternalProviderRuntimeError::Operation("invalid create identity".to_owned())
        })?;
        let launch = self
            .registry
            .begin_create(operation.clone(), self.connection)
            .map_err(|error| ExternalProviderRuntimeError::Operation(error.to_string()))?;
        let admitted = launch
            .owner
            .handle
            .try_submit(
                launch.owner.address,
                SessionOwnerCommand::Create {
                    operation: correlation,
                },
            )
            .map_err(|_| ExternalProviderRuntimeError::TransportFailure)?;
        let (response, result) = oneshot::channel();
        let stop = self.shutdown.child_token();
        let recover_roots = work_roots.clone();
        let provisional_operation = operation.clone();
        let ports = ProvisionalCreate {
            door: Arc::downgrade(self),
            endpoint,
            operation,
            input: Some((setup, requested)),
            work_roots,
            prepared: Arc::default(),
            rebase: None,
            settings: None,
            adopted: None,
            response: Some(response),
            stop: stop.clone(),
            configuring: None,
        };
        let door = Arc::downgrade(self);
        let incarnation = launch.owner.address.incarnation;
        self.tasks.spawn(async move {
            let outcome = launch.task.run(ports, stop).await;
            if let Err(error) = &outcome {
                tracing::debug!(%error, "ACP create owner stopped");
            }
            if let Some(door) = door.upgrade() {
                let owner = door
                    .sessions
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .iter()
                    .find(|(_, entry)| entry.owner.address.incarnation == incarnation)
                    .map(|(target, entry)| (target.clone(), entry.clone()));
                if let Some((target, owner)) = owner {
                    if outcome.is_err()
                        && let Some((task, ports, replacement)) =
                            door.recover_session(&target, &owner, recover_roots.clone())
                    {
                        door.run_registered(target, task, ports, replacement, recover_roots);
                    } else {
                        owner.calls.stop();
                    }
                } else {
                    let _retired = door
                        .registry
                        .retire_create(&provisional_operation, incarnation);
                }
            }
        });
        admitted
            .await
            .map_err(|_| ExternalProviderRuntimeError::TransportFailure)?
            .map_err(|reason| {
                ExternalProviderRuntimeError::Operation(format!(
                    "the session owner refused create: {reason:?}"
                ))
            })?;
        result
            .await
            .map_err(|_| ExternalProviderRuntimeError::TransportFailure)?
    }
}

impl ProvisionalCreate {
    fn start(
        &mut self,
        address: OwnerAddress,
        operation: Uuid,
        completions: &OwnerCompletionPort,
    ) -> Result<Option<OwnerEvent>, OwnerTaskError> {
        let door = self.door.upgrade().ok_or(OwnerTaskError::ExecutorStopped)?;
        let (setup, requested) = self.input.take().ok_or(OwnerTaskError::ExecutorStopped)?;
        let completion = completions.reserve(address)?;
        let receipt = match door.wire.create(address.incarnation, setup) {
            Ok(receipt) => receipt,
            Err(_) => {
                return Ok(Some(OwnerEvent::Create(CreateProgress::Failed {
                    operation,
                    remote: RemoteEffect::None,
                })));
            }
        };
        let prepared = self.prepared.clone();
        let stop = self.stop.clone();
        #[cfg(test)]
        let preparation_fixture = door.preparation_fixture.clone();
        door.tasks.spawn(async move {
            let acquired = tokio::select! {
                result = receipt => result,
                () = stop.cancelled() => return,
            };
            let result = match acquired {
                Ok(Ok(session)) => Ok(CreatePreparation::Opened(session, requested)),
                Ok(Err(error)) => Err(error),
                Err(_) => Err(ExternalProviderRuntimeError::TransportFailure),
            };
            let event = match &result {
                Ok(CreatePreparation::Opened(session, _)) => {
                    OwnerEvent::Create(CreateProgress::ProviderId {
                        operation,
                        provider_id: session.provider_id(),
                    })
                }
                Ok(CreatePreparation::Configured(_, _)) => return,
                Err(error) => OwnerEvent::Create(CreateProgress::Failed {
                    operation,
                    remote: if error
                        .coded_wire_failure()
                        .is_some_and(|failure| failure.definitely_no_effect())
                    {
                        RemoteEffect::None
                    } else {
                        RemoteEffect::Unknown
                    },
                }),
            };
            #[cfg(test)]
            if result.is_ok()
                && let Some(release) = preparation_fixture.hold_create()
            {
                tokio::select! {
                    released = release => { let _released = released; }
                    () = stop.cancelled() => return,
                }
            }
            save(&prepared, Some(result));
            completion.send(event);
        });
        Ok(None)
    }

    fn adopt(
        &mut self,
        address: OwnerAddress,
        operation: Uuid,
        provider_id: String,
        completions: &OwnerCompletionPort,
    ) -> Result<Option<OwnerEvent>, OwnerTaskError> {
        let door = self.door.upgrade().ok_or(OwnerTaskError::ExecutorStopped)?;
        let id = match SessionId::try_from(provider_id) {
            Ok(id) => id,
            Err(_) => {
                return Ok(Some(OwnerEvent::Create(CreateProgress::Failed {
                    operation,
                    remote: RemoteEffect::Applied,
                })));
            }
        };
        let target = SessionRef {
            endpoint: self.endpoint.clone(),
            session_id: id,
        };
        let Some(Ok(CreatePreparation::Opened(opened, requested))) = take(&self.prepared) else {
            return Err(OwnerTaskError::ExecutorStopped);
        };
        let mut sessions = door
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (registered, lane) =
            match door
                .registry
                .adopt(&self.operation, address.incarnation, target.clone())
            {
                Ok(adopted) => adopted,
                Err(crate::session_owner_registry::SessionOwnerRegistryError::AlreadyOwned) => {
                    return Ok(Some(OwnerEvent::Create(CreateProgress::AlreadyOwned {
                        operation,
                    })));
                }
                Err(_) => return Err(OwnerTaskError::ExecutorStopped),
            };
        let (owner, mut ports) = door
            .prepare_adopted(
                target.clone(),
                registered,
                Some(lane),
                None,
                self.work_roots.clone(),
            )
            .map_err(|_| OwnerTaskError::ExecutorStopped)?;
        if let Some(batch) = self.rebase.take()
            && let Some(result) =
                ports.execute(address, OwnerEffect::Publish(batch), completions)?
        {
            completions.reserve(address)?.send(result);
        }
        sessions.insert(target, owner);
        self.adopted = Some(ports);
        drop(sessions);
        self.configure(
            door.as_ref(),
            address,
            operation,
            opened,
            requested,
            completions,
        )?;
        Ok(None)
    }

    fn configure(
        &mut self,
        door: &SessionOwnerDoor,
        address: OwnerAddress,
        operation: Uuid,
        mut opened: OwnerOpenedSession,
        requested: RequestedProviderSettings,
        completions: &OwnerCompletionPort,
    ) -> Result<(), OwnerTaskError> {
        let completion = completions.reserve(address)?;
        let prepared_slot = self.prepared.clone();
        let clock = door.clock.clone();
        let stop = self.stop.child_token();
        self.configuring = Some(stop.clone());
        door.tasks.spawn(async move {
            let result = tokio::select! {
                biased;
                () = stop.cancelled() => return,
                result = opened.configure_create(&requested, clock.as_ref()) => result,
            };
            let (prepared, event) = match result {
                Ok(settings) => {
                    let facts = Box::new(settings.owner_facts(&opened));
                    (
                        Ok(CreatePreparation::Configured(opened, Box::new(settings))),
                        CreateProgress::Adopted { operation, facts },
                    )
                }
                Err(error) => (
                    Err(error),
                    CreateProgress::Failed {
                        operation,
                        remote: RemoteEffect::Applied,
                    },
                ),
            };
            save(&prepared_slot, Some(prepared));
            completion.send(OwnerEvent::Create(event));
        });
        Ok(())
    }

    fn reply(&mut self, address: OwnerAddress, outcome: CreateOutcome) {
        if let Some(configuring) = self.configuring.take() {
            configuring.cancel();
        }
        let result = match outcome {
            CreateOutcome::Adopted(_) => self
                .settings
                .take()
                .ok_or(ExternalProviderRuntimeError::TransportFailure),
            CreateOutcome::SessionIdInUse => Err(ExternalProviderRuntimeError::Operation(
                "this session id is already in use".to_owned(),
            )),
            CreateOutcome::Failed(_) => match take(&self.prepared) {
                Some(Err(error)) => Err(error),
                _ => Err(ExternalProviderRuntimeError::TransportFailure),
            },
        };
        if let Some(response) = self.response.take() {
            let _caller_gone = response.send(result);
        }
        if self.adopted.is_none() {
            if let Some(door) = self.door.upgrade() {
                let _retired = door
                    .registry
                    .retire_create(&self.operation, address.incarnation);
            }
            self.stop.cancel();
        }
    }
}

impl SessionOwnerPorts for ProvisionalCreate {
    fn execute(
        &mut self,
        address: OwnerAddress,
        effect: OwnerEffect,
        completions: &OwnerCompletionPort,
    ) -> Result<Option<OwnerEvent>, OwnerTaskError> {
        match effect {
            OwnerEffect::Wire(SessionWireRequest::Create { operation }) => {
                self.start(address, operation, completions)
            }
            OwnerEffect::AdoptProviderId {
                operation,
                provider_id,
            } => self.adopt(address, operation, provider_id, completions),
            OwnerEffect::CreateSettled { outcome, .. } => {
                self.reply(address, outcome);
                Ok(None)
            }
            OwnerEffect::Publish(batch) if self.adopted.is_none() => {
                self.rebase = Some(batch);
                Ok(None)
            }
            OwnerEffect::Publish(batch) => {
                let ports = self
                    .adopted
                    .as_mut()
                    .ok_or(OwnerTaskError::ExecutorStopped)?;
                if matches!(&batch, PublicationBatch::Commit { facts, .. } if facts.iter().any(|fact| {
                    matches!(fact, PublicationFact::Live(SessionPublication::Created(_)))
                })) {
                    let Some(Ok(CreatePreparation::Configured(opened, settings))) =
                        take(&self.prepared)
                    else {
                        return Err(OwnerTaskError::ExecutorStopped);
                    };
                    ports.prepare_created(opened);
                    self.settings = Some(*settings);
                }
                ports.execute(address, OwnerEffect::Publish(batch), completions)
            }
            other => self
                .adopted
                .as_mut()
                .ok_or(OwnerTaskError::ExecutorStopped)?
                .execute(address, other, completions),
        }
    }
}

fn take<T>(value: &Mutex<Option<T>>) -> Option<T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
}

fn save<T>(value: &Mutex<Option<T>>, result: Option<T>) {
    *value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = result;
}
