//! Observe real mail frames and lose one selected live publication without closing the socket.
use crate::{BOUND, FixtureResult};
use board_sync_protocol::{BoardConsumerProtocol, BoardDeliveryTypes};
use fleet_sync_protocol::{
    ClientFrame, DeliveryAnswer, DeliveryRequest, DeliveryTypes, FrameId, ItemId, OperationAnswer,
    OperationRequest, RecipientOutcome, ReportResult, ReportSeq, ServerCall, ServerFrame,
    ServerRequest, StatusCursor, StatusFetchResult, StatusPublication,
};
use std::sync::{Arc, Mutex};
use tokio::sync::watch;

type Acceptance = <BoardDeliveryTypes as DeliveryTypes>::Acceptance;

#[derive(Clone, Debug)]
pub enum MailWireEvent {
    Publication {
        publication: StatusPublication<Acceptance>,
        dropped: bool,
    },
    StatusFetched(StatusFetchResult<Acceptance>),
    StatusAcknowledged(StatusCursor),
    ReportSent {
        frame: FrameId,
        item: ItemId,
        sequence: ReportSeq,
        outcome: RecipientOutcome<Acceptance>,
    },
    ReportAnswered {
        frame: FrameId,
        answer: ReportResult<Acceptance>,
    },
}

struct ObservationState {
    drop_terminal: Option<ItemId>,
    events: watch::Sender<Vec<MailWireEvent>>,
}

#[derive(Clone, Default)]
pub(crate) struct MailWireObserver(Arc<Mutex<Option<ObservationState>>>);

pub struct MailWireObservation {
    owner: MailWireObserver,
    events: watch::Receiver<Vec<MailWireEvent>>,
}

impl MailWireObserver {
    pub(crate) fn begin(
        &self,
        drop_terminal: Option<ItemId>,
    ) -> FixtureResult<MailWireObservation> {
        let mut state = self.0.lock().map_err(|_| "mail wire observer poisoned")?;
        if state.is_some() {
            return Err("mail wire observations must not overlap".into());
        }
        let (events, waiting) = watch::channel(Vec::new());
        *state = Some(ObservationState {
            drop_terminal,
            events,
        });
        Ok(MailWireObservation {
            owner: self.clone(),
            events: waiting,
        })
    }

    pub(crate) fn client_frame(&self, bytes: &[u8]) -> FixtureResult {
        let state = self.0.lock().map_err(|_| "mail wire observer poisoned")?;
        let Some(state) = state.as_ref() else {
            return Ok(());
        };
        if let ClientFrame::Request {
            frame,
            body: OperationRequest::Delivery(request),
        } = serde_json::from_slice::<ClientFrame<BoardConsumerProtocol>>(bytes)?
        {
            match request {
                DeliveryRequest::Report { report, .. } => record(
                    state,
                    MailWireEvent::ReportSent {
                        frame,
                        item: report.item,
                        sequence: report.report_seq,
                        outcome: report.outcome,
                    },
                )?,
                DeliveryRequest::StatusAck { through, .. } => {
                    record(state, MailWireEvent::StatusAcknowledged(through))?
                }
                _ => {}
            }
        }
        Ok(())
    }

    pub(crate) fn server_frame(&self, bytes: &[u8]) -> FixtureResult<bool> {
        let state = self.0.lock().map_err(|_| "mail wire observer poisoned")?;
        let Some(state) = state.as_ref() else {
            return Ok(false);
        };
        match serde_json::from_slice::<ServerFrame<BoardConsumerProtocol>>(bytes)? {
            ServerFrame::Request {
                body: ServerRequest::Crate(ServerCall::PublishStatus { publication, .. }),
                ..
            } => {
                let dropped = state.drop_terminal == Some(publication.item)
                    && publication.status.is_terminal();
                record(
                    state,
                    MailWireEvent::Publication {
                        publication,
                        dropped,
                    },
                )?;
                return Ok(dropped);
            }
            ServerFrame::Answer {
                frame,
                body: OperationAnswer::Delivery(answer),
            } => match answer {
                DeliveryAnswer::StatusFetch(batch) => {
                    record(state, MailWireEvent::StatusFetched(batch))?
                }
                DeliveryAnswer::Report(answer) => {
                    record(state, MailWireEvent::ReportAnswered { frame, answer })?
                }
                _ => {}
            },
            _ => {}
        }
        Ok(false)
    }
}

fn record(state: &ObservationState, event: MailWireEvent) -> FixtureResult {
    if state.events.borrow().len() >= 4096 {
        return Err("mail wire observation exceeded its bounded history".into());
    }
    state.events.send_modify(|events| events.push(event));
    Ok(())
}

impl MailWireObservation {
    pub fn events(&self) -> Vec<MailWireEvent> {
        self.events.borrow().clone()
    }

    pub async fn wait_for(
        &mut self,
        predicate: impl Fn(&[MailWireEvent]) -> bool,
    ) -> FixtureResult {
        tokio::time::timeout(BOUND, self.events.wait_for(|events| predicate(events))).await??;
        Ok(())
    }
}

impl Drop for MailWireObservation {
    fn drop(&mut self) {
        if let Ok(mut state) = self.owner.0.lock() {
            *state = None;
        }
    }
}
