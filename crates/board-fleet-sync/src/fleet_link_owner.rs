//! One tracked persistence owner reads the native pump's authoritative snapshot.
use crate::machine_session_store::cache_error;
use board_sync_protocol::{
    FleetLinkFailure, FleetLinkFailureReason, FleetLinkRecord, FleetLinkState,
};
use chrono::Utc;
use fleet_protocol::PeerId;
use fleet_session::{ClientError, ConnectFailure, StoreError};
use fleet_session_client::{SessionDrop, SessionLinkEvent, SessionLinkState, SessionLinkStatus};
use message_board_storage::FleetCache;
use std::{sync::Arc, time::Duration};
use tokio::sync::{broadcast, mpsc, oneshot, watch};
#[path = "fleet_link_errors.rs"]
mod errors;
#[path = "fleet_link_session.rs"]
mod session;

const STATUS_READ_BOUND: Duration = Duration::from_secs(5);

#[derive(Clone)]
struct LinkInput {
    preparation: FleetLinkRecord,
    session: Option<watch::Receiver<SessionLinkState>>,
    returned: Option<ReturnedFailure>,
}

#[derive(Clone)]
struct ReturnedFailure {
    failure: FleetLinkFailure,
    instance: u64,
    last_drop: Option<SessionDrop>,
}

#[derive(Clone)]
pub struct FleetLinkPublisher {
    input: watch::Sender<LinkInput>,
}

#[derive(Clone)]
pub struct FleetLinkReader {
    barriers: mpsc::Sender<oneshot::Sender<FleetLinkRecord>>,
    committed: watch::Receiver<Option<FleetLinkRecord>>,
    completion: watch::Receiver<OwnerCompletion>,
}

enum OwnerCompletion {
    Running,
    Drained,
    Failed { message: String },
}

pub struct FleetLinkOwner {
    cache: Arc<FleetCache>,
    input: watch::Receiver<LinkInput>,
    barriers: mpsc::Receiver<oneshot::Sender<FleetLinkRecord>>,
    committed: watch::Sender<Option<FleetLinkRecord>>,
    completion: watch::Sender<OwnerCompletion>,
    accepted: Option<SessionLinkState>,
}

impl FleetLinkOwner {
    pub async fn prepare(
        cache: Arc<FleetCache>,
        peer: PeerId,
        endpoint: &str,
    ) -> Result<(FleetLinkPublisher, FleetLinkReader, Self), StoreError> {
        let previous = cache
            .fleet_link()
            .await
            .map_err(cache_error)?
            .filter(|record| record.peer == peer && record.endpoint == endpoint);
        let preparation = FleetLinkRecord {
            peer,
            endpoint: endpoint.to_owned(),
            instance: 0,
            connection: 0,
            revision: 0,
            last_frame_at: previous.as_ref().and_then(|record| record.last_frame_at),
            state: FleetLinkState::Disconnected { since: Utc::now() },
            last_drop: previous
                .as_ref()
                .and_then(|record| record.last_drop.clone()),
            last_failure: previous.and_then(|record| record.last_failure),
        };
        let (input, received) = watch::channel(LinkInput {
            preparation,
            session: None,
            returned: None,
        });
        let (barriers, requests) = mpsc::channel(16);
        let (committed, receipts) = watch::channel(None);
        let (completion, completed) = watch::channel(OwnerCompletion::Running);
        Ok((
            FleetLinkPublisher { input },
            FleetLinkReader {
                barriers,
                committed: receipts,
                completion: completed,
            },
            Self {
                cache,
                input: received,
                barriers: requests,
                committed,
                completion,
                accepted: None,
            },
        ))
    }

    pub async fn run(mut self) -> Result<(), StoreError> {
        let result = self.run_loop().await;
        self.completion.send_replace(match &result {
            Ok(()) => OwnerCompletion::Drained,
            Err(error) => {
                tracing::error!(event = "fleet.link.persistenceFailed", %error, "fleet.link.persistenceFailed");
                OwnerCompletion::Failed { message: error.to_string() }
            }
        });
        result
    }

    async fn run_loop(&mut self) -> Result<(), StoreError> {
        let mut native = None;
        let mut native_open = true;
        let initial = self.latest_record(&mut native, &mut native_open)?;
        self.persist(initial).await?;
        loop {
            tokio::select! {
                changed = self.input.changed() => {
                    let latest = self.latest_record(&mut native, &mut native_open)?;
                    self.persist(latest).await?;
                    if changed.is_err() { return Ok(()); }
                }
                open = session_changed(&mut native), if native_open => {
                    native_open = open;
                    let latest = self.latest_record(&mut native, &mut native_open)?;
                    self.persist(latest).await?;
                }
                Some(barrier) = self.barriers.recv() => {
                    // Read the library watch here, rather than waiting for a
                    // separately scheduled projector to catch up.
                    let latest = self.latest_record(&mut native, &mut native_open)?;
                    self.persist(latest.clone()).await?;
                    let _ = barrier.send(latest);
                }
            }
        }
    }

    fn latest_record(
        &mut self,
        native: &mut Option<watch::Receiver<SessionLinkState>>,
        native_open: &mut bool,
    ) -> Result<FleetLinkRecord, StoreError> {
        let input = self.input.borrow_and_update().clone();
        let Some(source) = input.session else {
            return Ok(input.preparation);
        };
        let current_instance = native.as_ref().map(|current| current.borrow().instance);
        let source_instance = source.borrow().instance;
        if current_instance != Some(source_instance) {
            *native = Some(source);
            *native_open = true;
        }
        let receiver = native.as_mut().ok_or_else(unavailable)?;
        let incoming = receiver.borrow_and_update().clone();
        let closed = receiver.has_changed().is_err();
        // Revisions are ordered only inside one pump instance. A new pump's
        // revision zero must supersede the previous pump's larger revision.
        if self.accepted.as_ref().is_none_or(|accepted| {
            incoming.instance != accepted.instance || incoming.revision >= accepted.revision
        }) {
            self.accepted = Some(incoming);
        }
        let accepted = self.accepted.as_ref().ok_or_else(unavailable)?;
        if closed
            && matches!(
                accepted.status,
                SessionLinkStatus::Connecting { .. } | SessionLinkStatus::Welcomed { .. }
            )
        {
            return Err(StoreError::Unavailable {
                message: "native session observation closed before terminal state".to_owned(),
            });
        }
        let mut preparation = input.preparation;
        if let Some(previous) = self.committed.borrow().as_ref() {
            preparation.last_drop = previous.last_drop.clone();
            preparation.last_failure = previous.last_failure.clone();
            preparation.last_frame_at = previous.last_frame_at;
        }
        let mut record = session::project_record(preparation, accepted)?;
        if let Some(returned) = input.returned
            && returned.instance == accepted.instance
            && returned.last_drop == accepted.last_drop
        {
            record.last_failure = Some(returned.failure);
        }
        Ok(record)
    }

    async fn persist(&self, record: FleetLinkRecord) -> Result<(), StoreError> {
        let changed = self.committed.borrow().as_ref() != Some(&record);
        if changed {
            self.cache
                .save_fleet_link(&record)
                .await
                .map_err(cache_error)?;
            self.committed.send_replace(Some(record));
        }
        Ok(())
    }
}

async fn session_changed(session: &mut Option<watch::Receiver<SessionLinkState>>) -> bool {
    match session {
        Some(session) => session.changed().await.is_ok(),
        None => std::future::pending().await,
    }
}

impl FleetLinkPublisher {
    pub async fn observe_events(events: broadcast::Receiver<SessionLinkEvent>) {
        session::observe_events(events).await;
    }

    pub fn attach_session(&self, session: watch::Receiver<SessionLinkState>) {
        self.input
            .send_modify(|input| input.session = Some(session));
    }

    pub fn preparation_failed(&self, message: String) {
        let reason = FleetLinkFailureReason::BoardPreparation { message };
        tracing::error!(event = "fleet.link.preparationFailed", details = ?reason, "fleet.link.preparationFailed");
        self.input.send_modify(|input| {
            input.preparation.state = FleetLinkState::Disconnected { since: Utc::now() };
            input.preparation.last_failure = Some(FleetLinkFailure {
                reason,
                at: Utc::now(),
            });
        });
    }

    pub(crate) fn log_connect_failure(error: &ConnectFailure) {
        let reason = errors::connect_failure(error);
        tracing::warn!(event = "fleet.link.connectFailed", reason = reason.as_str(), details = ?reason, "fleet.link.connectFailed");
    }

    pub fn session_failed(&self, error: &ClientError) {
        let reason = errors::client_failure(error);
        tracing::warn!(event = "fleet.link.sessionFailed", reason = reason.as_str(), details = ?reason, "fleet.link.sessionFailed");
        // These are native returned errors, never a second transport/state
        // producer. Logical status comes exclusively from its watch.
        self.input.send_modify(|input| {
            let failure = FleetLinkFailure {
                reason,
                at: Utc::now(),
            };
            match &input.session {
                Some(session) => {
                    let native = session.borrow();
                    input.returned = Some(ReturnedFailure {
                        failure,
                        instance: native.instance,
                        last_drop: native.last_drop.clone(),
                    });
                }
                None => input.preparation.last_failure = Some(failure),
            }
        });
    }

    pub fn session_stopped(&self, error: &ClientError) {
        self.session_failed(error);
    }
}

impl FleetLinkReader {
    pub async fn read(&self) -> Result<FleetLinkRecord, StoreError> {
        let (barrier, result) = oneshot::channel();
        tokio::time::timeout(STATUS_READ_BOUND, async {
            if self.barriers.send(barrier).await.is_err() {
                return self.final_record();
            }
            match result.await {
                Ok(record) => Ok(record),
                Err(_) => self.final_record(),
            }
        })
        .await
        .map_err(|_| unavailable())?
    }

    fn final_record(&self) -> Result<FleetLinkRecord, StoreError> {
        match &*self.completion.borrow() {
            OwnerCompletion::Drained => self.committed.borrow().clone().ok_or_else(unavailable),
            OwnerCompletion::Failed { message } => Err(StoreError::Unavailable {
                message: message.clone(),
            }),
            OwnerCompletion::Running => Err(unavailable()),
        }
    }
}

fn unavailable() -> StoreError {
    StoreError::Unavailable {
        message: "Fleet link owner is unavailable".to_owned(),
    }
}

#[cfg(test)]
#[path = "fleet_link_owner_test.rs"]
mod tests;
