use crate::ClientError;
use crate::{FailedCall, SendFailure, pending_calls::PendingCalls};
use central_link_protocol::{
    CorrelationId, DataFrame, LinkFrame, LinkRequest, ReplyPayload, RequestPayload,
};
use router_clock::{ClockInstant, RuntimeClock};
use std::{sync::Arc, time::Duration};
use workspace_model::{Caller, MachineId, central_mail::MessageRecord};

const PING_INTERVAL: Duration = Duration::from_secs(20);
const INITIAL_RETRY: Duration = Duration::from_secs(1);
const MAX_RETRY: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RefusalReason {
    CredentialRefused,
    MachineRevoked,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LinkStatus {
    Connecting,
    Connected { machine_id: MachineId },
    Refused { reason: RefusalReason },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConnectionAttempt(CorrelationId);

enum ConnectionState {
    Waiting {
        reconnect_at: ClockInstant,
    },
    AwaitingWelcome {
        attempt: ConnectionAttempt,
    },
    Connected {
        attempt: ConnectionAttempt,
        next_ping: ClockInstant,
        heartbeat: Heartbeat,
    },
    Refused {
        reason: RefusalReason,
    },
}
enum Heartbeat {
    Idle,
    AwaitingPong { first_sent_at: ClockInstant },
}

#[derive(Debug, Eq, PartialEq)]
pub enum ClientAction {
    Connect { attempt: ConnectionAttempt },
    SendPing,
    Reconnect { failed: Vec<FailedCall> },
}

#[derive(Debug, Eq, PartialEq)]
pub enum ClientEvent {
    Welcomed,
    Pong,
    Reply {
        correlation_id: CorrelationId,
        result: ReplyPayload,
    },
    Deliver {
        message: MessageRecord,
    },
}

pub struct LinkClient {
    clock: Arc<dyn RuntimeClock>,
    machine: MachineId,
    state: ConnectionState,
    retry_delay: Duration,
    pending: PendingCalls,
    id_context: uuid::ContextV7,
}

impl LinkClient {
    pub fn new(machine: MachineId, clock: Arc<dyn RuntimeClock>) -> Self {
        Self {
            state: ConnectionState::Waiting {
                reconnect_at: clock.monotonic_now(),
            },
            clock,
            machine,
            retry_delay: INITIAL_RETRY,
            pending: PendingCalls::default(),
            id_context: uuid::ContextV7::new(),
        }
    }

    pub fn status(&self) -> LinkStatus {
        match &self.state {
            ConnectionState::Waiting { .. } | ConnectionState::AwaitingWelcome { .. } => {
                LinkStatus::Connecting
            }
            ConnectionState::Connected { .. } => LinkStatus::Connected {
                machine_id: self.machine.clone(),
            },
            ConnectionState::Refused { reason } => LinkStatus::Refused { reason: *reason },
        }
    }

    /// The Host waits on this deadline through the same injected clock; no task is spawned here.
    pub fn next_deadline(&self) -> Option<ClockInstant> {
        match self.state {
            ConnectionState::Waiting { reconnect_at } => Some(reconnect_at),
            ConnectionState::Connected { next_ping, .. } => Some(next_ping),
            ConnectionState::AwaitingWelcome { .. } | ConnectionState::Refused { .. } => None,
        }
    }

    pub fn poll_time(&mut self) -> Option<ClientAction> {
        let now = self.clock.monotonic_now();
        match &mut self.state {
            ConnectionState::Waiting { reconnect_at } if now >= *reconnect_at => {
                let attempt = ConnectionAttempt(self.next_correlation());
                self.state = ConnectionState::AwaitingWelcome {
                    attempt: attempt.clone(),
                };
                Some(ClientAction::Connect { attempt })
            }
            ConnectionState::Connected {
                next_ping,
                heartbeat,
                ..
            } if now >= *next_ping => match heartbeat {
                Heartbeat::AwaitingPong { first_sent_at }
                    if now.saturating_duration_since(*first_sent_at)
                        >= PING_INTERVAL.saturating_mul(2) =>
                {
                    let failed = self.disconnect();
                    Some(ClientAction::Reconnect { failed })
                }
                Heartbeat::Idle => {
                    *heartbeat = Heartbeat::AwaitingPong { first_sent_at: now };
                    *next_ping = now.after(PING_INTERVAL);
                    Some(ClientAction::SendPing)
                }
                Heartbeat::AwaitingPong { .. } => {
                    *next_ping = now.after(PING_INTERVAL);
                    Some(ClientAction::SendPing)
                }
            },
            ConnectionState::Waiting { .. }
            | ConnectionState::Connected { .. }
            | ConnectionState::AwaitingWelcome { .. }
            | ConnectionState::Refused { .. } => None,
        }
    }

    pub fn begin_request(
        &mut self,
        caller: Caller,
        payload: RequestPayload,
    ) -> Result<LinkFrame, ClientError> {
        if !matches!(self.state, ConnectionState::Connected { .. }) {
            return Err(ClientError::CentralUnavailable);
        }
        let request = LinkRequest::new(self.next_correlation(), caller, payload)
            .map_err(|_| ClientError::InvalidRequest)?;
        self.pending.insert(request.clone())?;
        Ok(LinkFrame::Data(DataFrame::Request {
            request: Box::new(request),
        }))
    }

    /// Call before handing bytes to a socket write, since a failed write can be ambiguous.
    pub fn request_may_have_been_sent(
        &mut self,
        correlation: &CorrelationId,
    ) -> Result<(), ClientError> {
        self.pending.mark_may_have_been_sent(correlation)
    }

    /// Use NotSent only when the transport explicitly guarantees no request bytes were sent.
    pub fn send_failed(
        &mut self,
        attempt: &ConnectionAttempt,
        correlation: &CorrelationId,
        receipt: SendFailure,
    ) -> Result<FailedCall, ClientError> {
        self.check_attempt(attempt)?;
        self.pending.send_failed(correlation, receipt)
    }

    /// A protocol error is returned to the socket owner, which closes and calls `closed`.
    pub fn receive(
        &mut self,
        attempt: &ConnectionAttempt,
        frame: LinkFrame,
    ) -> Result<ClientEvent, ClientError> {
        self.check_attempt(attempt)?;
        if let LinkFrame::Data(DataFrame::Welcome { machine_id, .. }) = &frame {
            if !matches!(self.state, ConnectionState::AwaitingWelcome { .. })
                || machine_id != &self.machine
            {
                return Err(ClientError::InvalidWelcome);
            }
            self.state = ConnectionState::Connected {
                attempt: attempt.clone(),
                next_ping: self.clock.monotonic_now().after(PING_INTERVAL),
                heartbeat: Heartbeat::Idle,
            };
            self.retry_delay = INITIAL_RETRY;
            return Ok(ClientEvent::Welcomed);
        }
        if !matches!(self.state, ConnectionState::Connected { .. }) {
            return Err(ClientError::CentralUnavailable);
        }
        match frame {
            LinkFrame::Pong => {
                let ConnectionState::Connected { heartbeat, .. } = &mut self.state else {
                    return Err(ClientError::CentralUnavailable);
                };
                *heartbeat = Heartbeat::Idle;
                Ok(ClientEvent::Pong)
            }
            LinkFrame::Data(DataFrame::Reply {
                correlation_id,
                result,
            }) => {
                self.pending.complete(&correlation_id, &result)?;
                Ok(ClientEvent::Reply {
                    correlation_id,
                    result: *result,
                })
            }
            LinkFrame::Data(DataFrame::Deliver { message }) => {
                Ok(ClientEvent::Deliver { message: *message })
            }
            LinkFrame::Ping
            | LinkFrame::Data(
                DataFrame::Welcome { .. } | DataFrame::Request { .. } | DataFrame::Ack { .. },
            ) => Err(ClientError::UnexpectedFrame),
        }
    }

    pub fn closed(&mut self, attempt: &ConnectionAttempt) -> Result<Vec<FailedCall>, ClientError> {
        self.check_attempt(attempt)?;
        Ok(self.disconnect())
    }

    fn disconnect(&mut self) -> Vec<FailedCall> {
        if !matches!(self.state, ConnectionState::Refused { .. }) {
            self.state = ConnectionState::Waiting {
                reconnect_at: self.clock.monotonic_now().after(self.retry_delay),
            };
            self.retry_delay = self.retry_delay.saturating_mul(2).min(MAX_RETRY);
        }
        self.pending.fail_all()
    }

    pub fn refused(
        &mut self,
        attempt: &ConnectionAttempt,
        reason: RefusalReason,
    ) -> Result<Vec<FailedCall>, ClientError> {
        self.check_attempt(attempt)?;
        self.state = ConnectionState::Refused { reason };
        Ok(self.pending.fail_all())
    }

    fn check_attempt(&self, attempt: &ConnectionAttempt) -> Result<(), ClientError> {
        match &self.state {
            ConnectionState::AwaitingWelcome { attempt: current }
            | ConnectionState::Connected {
                attempt: current, ..
            } if current == attempt => Ok(()),
            ConnectionState::Waiting { .. }
            | ConnectionState::AwaitingWelcome { .. }
            | ConnectionState::Connected { .. }
            | ConnectionState::Refused { .. } => Err(ClientError::StaleConnection),
        }
    }

    fn next_correlation(&mut self) -> CorrelationId {
        let millis = u64::try_from(self.clock.utc_now().timestamp_millis()).unwrap_or(0);
        let nanos = u32::try_from(millis % 1000)
            .unwrap_or(0)
            .saturating_mul(1_000_000);
        CorrelationId::mint(uuid::Timestamp::from_unix(
            &self.id_context,
            millis / 1000,
            nanos,
        ))
    }

    pub fn reenrolled(&mut self) {
        if matches!(self.state, ConnectionState::Refused { .. }) {
            self.retry_delay = INITIAL_RETRY;
            self.state = ConnectionState::Waiting {
                reconnect_at: self.clock.monotonic_now(),
            };
        }
    }
}

#[cfg(test)]
#[path = "link_state_test.rs"]
mod tests;
