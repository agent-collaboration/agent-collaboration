//! Delay one selected real lease notice while operation answers continue.
use crate::FixtureResult;
use board_sync_protocol::{BoardConsumerProtocol, BoardNotice, PositionedBoardNotice};
use fleet_sync_protocol::{Epoch, Notice, NoticePosition, ServerFrame};
use message_board::ProjectId;
use std::sync::{Arc, Mutex};
use tokio::sync::watch;

enum GateStage {
    Awaiting,
    Held,
    Released,
}
enum LeaseNoticeKind {
    Granted,
    Superseded,
}

struct Selection {
    generation: u64,
    project: ProjectId,
    epoch: Epoch,
    kind: LeaseNoticeKind,
    stage: GateStage,
    observed: watch::Sender<Option<NoticePosition>>,
}
#[derive(Default)]
struct GateState {
    generation: u64,
    selected: Option<Selection>,
}
#[derive(Clone)]
pub(crate) struct LeaseNoticeGate {
    state: Arc<Mutex<GateState>>,
    changed: watch::Sender<()>,
}
impl Default for LeaseNoticeGate {
    fn default() -> Self {
        Self {
            state: Arc::new(Mutex::new(GateState::default())),
            changed: watch::channel(()).0,
        }
    }
}

pub struct LeaseNoticeObservation {
    gate: LeaseNoticeGate,
    generation: u64,
    observed: watch::Receiver<Option<NoticePosition>>,
}
impl LeaseNoticeGate {
    pub(crate) fn begin(
        &self,
        project: ProjectId,
        epoch: Epoch,
    ) -> FixtureResult<LeaseNoticeObservation> {
        self.begin_notice(project, epoch, LeaseNoticeKind::Granted)
    }

    pub(crate) fn begin_superseded(
        &self,
        project: ProjectId,
        epoch: Epoch,
    ) -> FixtureResult<LeaseNoticeObservation> {
        self.begin_notice(project, epoch, LeaseNoticeKind::Superseded)
    }

    fn begin_notice(
        &self,
        project: ProjectId,
        epoch: Epoch,
        kind: LeaseNoticeKind,
    ) -> FixtureResult<LeaseNoticeObservation> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "grant notice gate poisoned")?;
        if state
            .selected
            .as_ref()
            .is_some_and(|selection| !matches!(selection.stage, GateStage::Released))
        {
            return Err("grant notice gates must not overlap".into());
        }
        state.generation = state
            .generation
            .checked_add(1)
            .ok_or("grant notice generation exhausted")?;
        let generation = state.generation;
        let (observed, waiting) = watch::channel(None);
        state.selected = Some(Selection {
            generation,
            project,
            epoch,
            kind,
            stage: GateStage::Awaiting,
            observed,
        });
        self.changed.send_replace(());
        Ok(LeaseNoticeObservation {
            gate: self.clone(),
            generation,
            observed: waiting,
        })
    }

    pub(crate) fn subscribe(&self) -> watch::Receiver<()> {
        self.changed.subscribe()
    }

    pub(crate) fn defer(&self, bytes: &[u8]) -> FixtureResult<Option<u64>> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "grant notice gate poisoned")?;
        let Some(selected) = state.selected.as_mut() else {
            return Ok(None);
        };
        if matches!(selected.stage, GateStage::Released) {
            return Ok(None);
        }
        let ServerFrame::Notice { position, body } =
            serde_json::from_slice::<ServerFrame<BoardConsumerProtocol>>(bytes)?
        else {
            return Ok(None);
        };
        if matches!(selected.stage, GateStage::Awaiting) {
            let matches = match (&selected.kind, body) {
                (
                    LeaseNoticeKind::Granted,
                    Notice::Consumer(PositionedBoardNotice {
                        kind: BoardNotice::LeaseGranted { project, epoch },
                        ..
                    }),
                )
                | (
                    LeaseNoticeKind::Superseded,
                    Notice::Consumer(PositionedBoardNotice {
                        kind: BoardNotice::LeaseSuperseded { project, epoch },
                        ..
                    }),
                ) => project == selected.project && epoch == selected.epoch,
                _ => false,
            };
            if !matches {
                return Ok(None);
            }
            selected.stage = GateStage::Held;
            selected.observed.send_replace(Some(position));
        }
        // Preserve notice order behind the selected grant. Answers and server
        // requests remain independent, so fixture setup cannot deadlock Claim.
        Ok(Some(selected.generation))
    }

    pub(crate) fn is_held(&self, generation: u64) -> FixtureResult<bool> {
        let state = self
            .state
            .lock()
            .map_err(|_| "grant notice gate poisoned")?;
        Ok(state.selected.as_ref().is_some_and(|selected| {
            selected.generation == generation && matches!(selected.stage, GateStage::Held)
        }))
    }
}

impl LeaseNoticeObservation {
    pub async fn wait_held(&mut self) -> FixtureResult<NoticePosition> {
        self.observed
            .wait_for(Option::is_some)
            .await?
            .ok_or_else(|| "grant notice was not held".into())
    }

    pub fn release(&self) -> FixtureResult {
        let mut state = self
            .gate
            .state
            .lock()
            .map_err(|_| "grant notice gate poisoned")?;
        let selected = state
            .selected
            .as_mut()
            .filter(|selected| selected.generation == self.generation)
            .ok_or("grant notice observation ended")?;
        if !matches!(selected.stage, GateStage::Held) {
            return Err("grant notice has not reached its hold".into());
        }
        selected.stage = GateStage::Released;
        self.gate.changed.send_replace(());
        Ok(())
    }
}

impl Drop for LeaseNoticeObservation {
    fn drop(&mut self) {
        if let Ok(mut state) = self.gate.state.lock()
            && state
                .selected
                .as_ref()
                .is_some_and(|selected| selected.generation == self.generation)
        {
            state.selected = None;
            self.gate.changed.send_replace(());
        }
    }
}
