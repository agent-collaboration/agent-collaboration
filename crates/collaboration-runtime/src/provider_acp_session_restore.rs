//! Connection-owned restore requests and the frames that arrive during them.

use codex_acp_adapter::QueuedAcpFrame;
use collaboration_contracts::{SessionCommandPort, SessionEventHub, SessionTargetCommand};
use message_board::{Identity, SessionRef};
use router_clock::{ClockInstant, RuntimeClock};
use serde_json::Value;
use session_event_model::SessionState;
use std::{collections::VecDeque, future::Future};

use super::{STORE_BUDGET, command_failure, failure, response, session_result};
use crate::provider_acp_event_projection::PreparedProviderObservation;

pub(super) const STORE_ELAPSED: (i32, &str) = (-32000, "the store didn't respond in time");

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum AcpRestoreMethod {
    Load,
    Resume,
}

impl AcpRestoreMethod {
    pub(super) fn from_rpc(method: &str) -> Option<Self> {
        match method {
            "session/load" => Some(Self::Load),
            "session/resume" => Some(Self::Resume),
            _ => None,
        }
    }
}

pub(super) const RESTORE_IN_PROGRESS: (i32, &str) = (
    -32000,
    "session was loading when this request arrived; check the first restore reply before retrying",
);

pub(super) enum WaitingSessionFrame {
    Command(QueuedAcpFrame),
    DuplicateLoad {
        frame: QueuedAcpFrame,
        session: SessionRef,
        cwd: String,
        valid_refusal: (i32, &'static str),
    },
    DuplicateResume {
        frame: QueuedAcpFrame,
        refusal: (i32, &'static str),
    },
    RestoreRefusal {
        frame: QueuedAcpFrame,
        code: i32,
        message: &'static str,
    },
}

/// Entered when the route starts restore validation. Released only after the
/// reply is queued, so later frames run in arrival order. Keeping their
/// input frames also keeps the connection's existing byte-budget permits.
#[derive(Default)]
pub(super) struct AwaitingRestore {
    pub(super) frames: VecDeque<WaitingSessionFrame>,
}

impl AwaitingRestore {
    pub(super) fn ready_after(
        mut self,
        post_activation_elapsed: bool,
    ) -> VecDeque<WaitingSessionFrame> {
        if post_activation_elapsed {
            for frame in &mut self.frames {
                match frame {
                    WaitingSessionFrame::DuplicateLoad { valid_refusal, .. }
                    | WaitingSessionFrame::DuplicateResume {
                        refusal: valid_refusal,
                        ..
                    } => {
                        *valid_refusal = STORE_ELAPSED;
                    }
                    WaitingSessionFrame::Command(_)
                    | WaitingSessionFrame::RestoreRefusal { .. } => {}
                }
            }
        }
        self.frames
    }
}

pub(super) struct RestoreRequest {
    pub(super) method: AcpRestoreMethod,
    pub(super) session: SessionRef,
    pub(super) id: Value,
    pub(super) cwd: Option<String>,
}

pub(super) enum RestoreCompletion {
    Activated(RestoreRequest),
    Reply {
        session: SessionRef,
        reply: Value,
    },
    Observed {
        session: SessionRef,
        reply: Value,
        observation: Box<PreparedProviderObservation>,
    },
    PostActivationElapsed {
        session: SessionRef,
        id: Value,
    },
}

impl RestoreRequest {
    pub(super) async fn activate(
        self,
        commands: &dyn SessionCommandPort,
        events: &dyn SessionEventHub,
        clock: &dyn RuntimeClock,
        actor: Identity,
    ) -> RestoreCompletion {
        let deadline = clock.monotonic_now().after(STORE_BUDGET);
        if let Some(cwd) = self.cwd.as_deref()
            && let Err((code, message)) =
                validate_load_inventory(events, clock, deadline, &self.session, cwd).await
        {
            return RestoreCompletion::Reply {
                session: self.session,
                reply: failure(self.id, code, message),
            };
        }
        match router_clock::timeout_at(clock, deadline, events.state(self.session.clone())).await {
            Ok(Ok(SessionState::Unloaded)) => {}
            Ok(Ok(_)) => return RestoreCompletion::Activated(self),
            Ok(Err(_)) => {
                return RestoreCompletion::Reply {
                    session: self.session,
                    reply: failure(self.id, -32002, "Session not found"),
                };
            }
            Err(_) => {
                return RestoreCompletion::Reply {
                    session: self.session,
                    reply: failure(self.id, STORE_ELAPSED.0, STORE_ELAPSED.1),
                };
            }
        }
        let command = SessionTargetCommand {
            target: self.session.clone(),
            actor,
        };
        let outcome = match self.method {
            AcpRestoreMethod::Load => commands.load_session(command).await,
            AcpRestoreMethod::Resume => commands.resume_session(command).await,
        };
        match outcome {
            Ok(()) => RestoreCompletion::Activated(self),
            Err(error) => RestoreCompletion::Reply {
                session: self.session,
                reply: command_failure(self.id, error),
            },
        }
    }

    pub(super) async fn reply(
        self,
        events: &dyn SessionEventHub,
        clock: &dyn RuntimeClock,
        prepare: impl Future<Output = Result<PreparedProviderObservation, ()>>,
    ) -> RestoreCompletion {
        let history_unavailable = self.method == AcpRestoreMethod::Resume;
        let result = router_clock::timeout(clock, STORE_BUDGET, async {
            let observation = prepare
                .await
                .map_err(|()| "Session event hub unavailable")?;
            let payload = session_result(
                events,
                &self.session,
                observation.report.clone(),
                history_unavailable,
            )
            .await
            .map_err(|()| "Session inventory unavailable")?;
            Ok((observation, payload))
        })
        .await;
        match result {
            Ok(Ok((observation, payload))) => RestoreCompletion::Observed {
                session: self.session,
                reply: response(self.id, payload),
                observation: Box::new(observation),
            },
            Ok(Err(message)) => RestoreCompletion::Reply {
                session: self.session,
                reply: failure(self.id, -32000, message),
            },
            Err(_) => RestoreCompletion::PostActivationElapsed {
                session: self.session,
                id: self.id,
            },
        }
    }
}

pub(super) async fn reject_duplicate_load(
    events: &dyn SessionEventHub,
    clock: &dyn RuntimeClock,
    session: SessionRef,
    id: Value,
    cwd: String,
    valid_refusal: (i32, &'static str),
) -> RestoreCompletion {
    let deadline = clock.monotonic_now().after(STORE_BUDGET);
    let (code, message) = validate_load_inventory(events, clock, deadline, &session, &cwd)
        .await
        .err()
        .unwrap_or(valid_refusal);
    RestoreCompletion::Reply {
        session,
        reply: failure(id, code, message),
    }
}

async fn validate_load_inventory(
    events: &dyn SessionEventHub,
    clock: &dyn RuntimeClock,
    deadline: ClockInstant,
    session: &SessionRef,
    cwd: &str,
) -> Result<(), (i32, &'static str)> {
    let summaries =
        router_clock::timeout_at(clock, deadline, events.sessions(session.endpoint.clone()))
            .await
            .map_err(|_| STORE_ELAPSED)?
            .map_err(|_| (-32000, "Session inventory unavailable"))?;
    let stored = summaries
        .into_iter()
        .find(|summary| summary.session == *session)
        .ok_or((-32002, "Session not found"))?;
    if stored.working_directory.to_str() != Some(cwd) {
        return Err((-32602, "Session working directory does not match"));
    }
    Ok(())
}
