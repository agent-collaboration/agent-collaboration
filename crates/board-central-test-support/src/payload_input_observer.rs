//! Observe opaque input bodies in real replies without decoding their contents.
use crate::FixtureResult;
use board_sync_protocol::{
    BoardConsumerAnswer, BoardConsumerProtocol, BoardReadPage, FleetFeedRead,
};
use fleet_sync_protocol::{OperationAnswer, ServerFrame};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RouterPayloadInputBodies {
    pub fleet_records: u64,
    pub fleet_decisions: u64,
}

#[derive(Default)]
enum ObservationState {
    #[default]
    Idle,
    Measuring(Result<RouterPayloadInputBodies, String>),
}

#[derive(Clone, Default)]
pub(crate) struct PayloadInputObserver(Arc<Mutex<ObservationState>>);

pub struct PayloadInputObservation {
    observer: Option<PayloadInputObserver>,
}

impl PayloadInputObserver {
    pub(crate) fn begin(&self) -> FixtureResult<PayloadInputObservation> {
        let mut state = self.0.lock().map_err(|_| "payload observer poisoned")?;
        if !matches!(*state, ObservationState::Idle) {
            return Err("payload input observations must not overlap".into());
        }
        *state = ObservationState::Measuring(Ok(RouterPayloadInputBodies::default()));
        Ok(PayloadInputObservation {
            observer: Some(self.clone()),
        })
    }

    pub(crate) fn observe(&self, bytes: &[u8]) -> FixtureResult {
        let mut state = self.0.lock().map_err(|_| "payload observer poisoned")?;
        let ObservationState::Measuring(result) = &mut *state else {
            return Ok(());
        };
        let Ok(counts) = result else {
            return Ok(());
        };
        match serde_json::from_slice::<ServerFrame<BoardConsumerProtocol>>(bytes) {
            Ok(ServerFrame::Answer {
                body: OperationAnswer::Consumer(answer),
                ..
            }) => match answer {
                BoardConsumerAnswer::Fleet { .. } => counts.fleet_decisions += 1,
                BoardConsumerAnswer::Read(page) => match page {
                    BoardReadPage::FleetFeed(FleetFeedRead::Page { records, .. })
                    | BoardReadPage::FleetSnapshot { records, .. } => {
                        counts.fleet_records += u64::try_from(records.len())?;
                    }
                    BoardReadPage::Directory { entries, .. } => {
                        counts.fleet_records += u64::try_from(entries.len())?;
                    }
                    _ => {}
                },
                _ => {}
            },
            Ok(_) => {}
            Err(error) => *result = Err(format!("payload input reply did not parse: {error}")),
        }
        Ok(())
    }
}

impl PayloadInputObservation {
    pub fn finish(mut self) -> FixtureResult<RouterPayloadInputBodies> {
        let observer = self.observer.take().ok_or("observation already finished")?;
        let mut state = observer.0.lock().map_err(|_| "payload observer poisoned")?;
        match std::mem::take(&mut *state) {
            ObservationState::Measuring(counts) => counts.map_err(Into::into),
            ObservationState::Idle => Err("payload observation was not active".into()),
        }
    }
}

impl Drop for PayloadInputObservation {
    fn drop(&mut self) {
        if let Some(observer) = &self.observer
            && let Ok(mut state) = observer.0.lock()
        {
            *state = ObservationState::Idle;
        }
    }
}
