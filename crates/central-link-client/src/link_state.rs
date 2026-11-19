use crate::ClientError;
use crate::{
    FailedCall, SendFailure,
    pending_calls::{PendingCalls, ReplyDisposition},
};
use central_link_protocol::{
    CorrelationId, DataFrame, LinkFrame, LinkRequest, ReplyPayload, RequestPayload,
    RequestStatusAnswer, RequestStatusQuery, TokenRenewalResult,
};
use router_clock::{ClockInstant, RuntimeClock};
use std::{sync::Arc, time::Duration};
use workspace_ids::MachineId;
use workspace_model::{
    Caller, MessageId, ResolvedSender,
    central_mail::{DeliveryAnswer, MessageRecord},
};

const PING_INTERVAL: Duration = Duration::from_secs(20);
const CALL_DEADLINE: Duration = Duration::from_secs(30);
const WELCOME_DEADLINE: Duration = Duration::from_secs(10);
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
        last_attempt: Option<ConnectionAttempt>,
    },
    AwaitingWelcome {
        attempt: ConnectionAttempt,
        expires_at: ClockInstant,
    },
    Connected {
        attempt: ConnectionAttempt,
        next_ping: ClockInstant,
        heartbeat: Heartbeat,
    },
    Refused {
        attempt: ConnectionAttempt,
        reason: RefusalReason,
    },
}
enum Heartbeat {
    Idle,
    OneUnansweredProbe,
    TwoUnansweredProbes,
}

#[derive(Debug, Eq, PartialEq)]
pub enum ClientAction {
    Connect { attempt: ConnectionAttempt },
    SendPing,
    Reconnect { failed: Vec<FailedCall> },
    CallsExpired { failed: Vec<FailedCall> },
}

#[derive(Debug, Eq, PartialEq)]
pub enum ClientEvent {
    Welcomed,
    Pong,
    Reply {
        correlation_id: CorrelationId,
        result: ReplyPayload,
    },
    RequestStatusReply {
        correlation_id: CorrelationId,
        result: RequestStatusAnswer,
    },
    LateReply {
        correlation_id: CorrelationId,
    },
    /// A message central delivered to this Router (2c); repeats are safe.
    Deliver {
        message: MessageRecord,
        sender: ResolvedSender,
    },
    /// Central's answer to a RenewMachineToken the Host sent.
    TokenRenewal {
        correlation_id: CorrelationId,
        result: TokenRenewalResult,
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
                last_attempt: None,
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
            ConnectionState::Refused { reason, .. } => LinkStatus::Refused { reason: *reason },
        }
    }

    /// The Host drives this deadline through the same clock; no task is spawned here.
    pub fn next_deadline(&self) -> Option<ClockInstant> {
        let connection = match self.state {
            ConnectionState::Waiting { reconnect_at, .. } => Some(reconnect_at),
            ConnectionState::AwaitingWelcome { expires_at, .. } => Some(expires_at),
            ConnectionState::Connected { next_ping, .. } => Some(next_ping),
            ConnectionState::Refused { .. } => None,
        };
        connection
            .into_iter()
            .chain(self.pending.next_deadline())
            .min()
    }

    /// Drain due actions before processing socket events; call again until None.
    pub fn poll_time(&mut self) -> Option<ClientAction> {
        let now = self.clock.monotonic_now();
        let failed = self.pending.expire(now);
        if !failed.is_empty() {
            return Some(ClientAction::CallsExpired { failed });
        }
        match &mut self.state {
            ConnectionState::Waiting { reconnect_at, .. } if now >= *reconnect_at => {
                let attempt = ConnectionAttempt(self.next_correlation());
                self.pending.begin_connection();
                self.state = ConnectionState::AwaitingWelcome {
                    attempt: attempt.clone(),
                    expires_at: now.after(WELCOME_DEADLINE),
                };
                Some(ClientAction::Connect { attempt })
            }
            ConnectionState::AwaitingWelcome { expires_at, .. } if now >= *expires_at => {
                let failed = self.disconnect();
                Some(ClientAction::Reconnect { failed })
            }
            ConnectionState::Connected {
                next_ping,
                heartbeat,
                ..
            } if now >= *next_ping => match heartbeat {
                Heartbeat::TwoUnansweredProbes => {
                    let failed = self.disconnect();
                    Some(ClientAction::Reconnect { failed })
                }
                Heartbeat::Idle => {
                    *heartbeat = Heartbeat::OneUnansweredProbe;
                    *next_ping = now.after(PING_INTERVAL);
                    Some(ClientAction::SendPing)
                }
                Heartbeat::OneUnansweredProbe => {
                    *heartbeat = Heartbeat::TwoUnansweredProbes;
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
        self.pending.insert(
            request.clone(),
            self.clock.monotonic_now().after(CALL_DEADLINE),
        )?;
        Ok(LinkFrame::Data(DataFrame::Request {
            request: Box::new(request),
        }))
    }

    /// A delivery Ack carries no correlation id and expects no reply.
    pub fn acknowledge_delivery(
        &self,
        message_id: MessageId,
        answer: DeliveryAnswer,
    ) -> Result<LinkFrame, ClientError> {
        if !matches!(self.state, ConnectionState::Connected { .. }) {
            return Err(ClientError::CentralUnavailable);
        }
        Ok(LinkFrame::Data(DataFrame::Ack { message_id, answer }))
    }

    pub fn begin_request_status(
        &mut self,
        caller: Caller,
        query: RequestStatusQuery,
    ) -> Result<LinkFrame, ClientError> {
        if !matches!(self.state, ConnectionState::Connected { .. }) {
            return Err(ClientError::CentralUnavailable);
        }
        let correlation_id = self.next_correlation();
        self.pending.insert_request_status(
            correlation_id.clone(),
            self.clock.monotonic_now().after(CALL_DEADLINE),
        )?;
        Ok(LinkFrame::Data(DataFrame::RequestStatus {
            correlation_id,
            caller,
            query,
        }))
    }

    /// Only DefinitelyNotSent guarantees no request bytes were sent.
    pub fn send_failed(
        &mut self,
        attempt: &ConnectionAttempt,
        correlation: &CorrelationId,
        receipt: SendFailure,
    ) -> Result<FailedCall, ClientError> {
        self.check_attempt(attempt)?;
        self.pending.send_failed(correlation, receipt)
    }

    /// Protocol errors require close; a typed LateReply event does not.
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
            return Err(ClientError::UnexpectedFrame);
        }
        match frame {
            LinkFrame::Pong => {
                let ConnectionState::Connected { heartbeat, .. } = &mut self.state else {
                    return Err(ClientError::UnexpectedFrame);
                };
                *heartbeat = Heartbeat::Idle;
                Ok(ClientEvent::Pong)
            }
            LinkFrame::Data(DataFrame::Reply {
                correlation_id,
                result,
            }) => match self.pending.complete(&correlation_id, &result)? {
                ReplyDisposition::Completed => Ok(ClientEvent::Reply {
                    correlation_id,
                    result: *result,
                }),
                ReplyDisposition::AlreadyResolved => Ok(ClientEvent::LateReply { correlation_id }),
            },
            LinkFrame::Data(DataFrame::Deliver { message, sender }) => Ok(ClientEvent::Deliver {
                message: *message,
                sender,
            }),
            LinkFrame::Data(DataFrame::RequestStatusReply {
                correlation_id,
                result,
            }) => match self.pending.complete_request_status(&correlation_id)? {
                ReplyDisposition::Completed => Ok(ClientEvent::RequestStatusReply {
                    correlation_id,
                    result: *result,
                }),
                ReplyDisposition::AlreadyResolved => Ok(ClientEvent::LateReply { correlation_id }),
            },
            // The Host owns renewal correlation (it sent the frame); the client
            // only surfaces central's answer.
            LinkFrame::Data(DataFrame::MachineTokenRenewal {
                correlation_id,
                result,
            }) => Ok(ClientEvent::TokenRenewal {
                correlation_id,
                result,
            }),
            LinkFrame::Ping
            | LinkFrame::Data(
                DataFrame::Welcome { .. }
                | DataFrame::Request { .. }
                | DataFrame::RequestStatus { .. }
                | DataFrame::Batch { .. }
                | DataFrame::Ack { .. }
                | DataFrame::RenewMachineToken { .. },
            ) => Err(ClientError::UnexpectedFrame),
        }
    }

    pub fn closed(&mut self, attempt: &ConnectionAttempt) -> Result<Vec<FailedCall>, ClientError> {
        self.check_most_recent_attempt(attempt)?;
        match self.state {
            ConnectionState::AwaitingWelcome { .. } | ConnectionState::Connected { .. } => {
                Ok(self.disconnect())
            }
            ConnectionState::Waiting { .. } | ConnectionState::Refused { .. } => Ok(Vec::new()),
        }
    }

    fn disconnect(&mut self) -> Vec<FailedCall> {
        let last_attempt = self.most_recent_attempt().cloned();
        self.state = ConnectionState::Waiting {
            reconnect_at: self.clock.monotonic_now().after(self.retry_delay),
            last_attempt,
        };
        self.retry_delay = self.retry_delay.saturating_mul(2).min(MAX_RETRY);
        self.pending.fail_all()
    }

    /// A refusal may precede or follow closed for the latest attempt, but never a newer one.
    pub fn refused(
        &mut self,
        attempt: &ConnectionAttempt,
        reason: RefusalReason,
    ) -> Result<Vec<FailedCall>, ClientError> {
        self.check_most_recent_attempt(attempt)?;
        self.state = ConnectionState::Refused {
            attempt: attempt.clone(),
            reason,
        };
        Ok(self.pending.fail_all())
    }

    fn check_attempt(&self, attempt: &ConnectionAttempt) -> Result<(), ClientError> {
        match &self.state {
            ConnectionState::AwaitingWelcome {
                attempt: current, ..
            }
            | ConnectionState::Connected {
                attempt: current, ..
            } if current == attempt => Ok(()),
            ConnectionState::Waiting { .. }
            | ConnectionState::AwaitingWelcome { .. }
            | ConnectionState::Connected { .. }
            | ConnectionState::Refused { .. } => Err(ClientError::StaleConnection),
        }
    }

    fn most_recent_attempt(&self) -> Option<&ConnectionAttempt> {
        match &self.state {
            ConnectionState::Waiting { last_attempt, .. } => last_attempt.as_ref(),
            ConnectionState::AwaitingWelcome { attempt, .. }
            | ConnectionState::Connected { attempt, .. }
            | ConnectionState::Refused { attempt, .. } => Some(attempt),
        }
    }

    fn check_most_recent_attempt(&self, attempt: &ConnectionAttempt) -> Result<(), ClientError> {
        if self.most_recent_attempt() == Some(attempt) {
            Ok(())
        } else {
            Err(ClientError::StaleConnection)
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
                last_attempt: None,
            };
        }
    }
}

#[cfg(test)]
#[path = "link_state_test.rs"]
mod tests;
