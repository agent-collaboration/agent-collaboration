use crate::copy_storage::{encode_record, statement, text};
use crate::registry_contract::PendingContinuation;
use crate::registry_storage::{read_record, session_dispatch};
use crate::session_reply::SessionReplyKind;
use crate::*;
use chrono::{DateTime, Utc};
use collaboration_replication_protocol::*;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SessionRecord {
    pub installation: InstallationId,
    pub session_id: SessionId,
    pub connection: ConnectionId,
    pub expires_at: DateTime<Utc>,
}
pub(crate) fn bootstrap(tx: &mut dyn SqlTx) -> Result<(), CentralError> {
    for sql in [
        include_str!("../migrations/0003_session_storage.sql"),
        include_str!("../migrations/0009_session_notices.sql"),
    ]
    .join(";")
    .split(';')
    .map(str::trim)
    .filter(|s| !s.is_empty())
    {
        tx.execute(statement(sql, Vec::new()))?;
    }
    Ok(())
}
pub(crate) fn load_session(tx: &mut dyn SqlTx) -> Result<Option<SessionRecord>, CentralError> {
    read_record(
        tx,
        "SELECT record FROM _session_state WHERE singleton=?",
        "1",
    )
}
fn save_session(tx: &mut dyn SqlTx, s: &SessionRecord) -> Result<(), CentralError> {
    tx.execute(statement("INSERT INTO _session_state(singleton,record) VALUES(1,?) ON CONFLICT(singleton) DO UPDATE SET record=excluded.record",vec![text(encode_record(s)?)]))?;
    Ok(())
}
fn send(to: &ConnectionId, frame: CentralFrame) -> SessionEffect {
    SessionEffect::Send {
        to: to.clone(),
        frame,
    }
}
fn close(to: &ConnectionId, reason: CredentialRejection) -> SessionEffect {
    send(
        to,
        CentralFrame::Closed {
            reason: SessionCloseReason::CredentialRejected(reason),
        },
    )
}
pub(crate) fn current(
    tx: &mut dyn SqlTx,
    cid: &ConnectionId,
    now: RegistryTime,
) -> Result<bool, CentralError> {
    Ok(load_session(tx)?.is_some_and(|s| s.connection == *cid && s.expires_at > now.0))
}
pub(crate) fn input(
    tx: &mut dyn SqlTx,
    input: &SessionInput,
    now: RegistryTime,
    ids: &mut IdBatch,
) -> Result<Vec<SessionEffect>, CentralError> {
    bootstrap(tx)?;
    match input {
        SessionInput::DecodeFailure {
            connection,
            failure,
        } => {
            if let Some(session) = load_session(tx)? {
                if session.connection != *connection {
                    return Ok(Vec::new());
                }
                if session.expires_at <= now.0 {
                    let mut effects = disconnect(tx, &session, ids)?;
                    effects.insert(0, close(connection, CredentialRejection::Expired));
                    return Ok(effects);
                }
            }
            let frame = match failure {
                MachineFrameDecodeFailure::InvalidRequest { frame_id, field } => {
                    CentralFrame::Answer {
                        frame_id: frame_id.clone(),
                        body: SessionAnswer::Refused(SessionRefusal::InvalidShape {
                            field: field.clone(),
                        }),
                    }
                }
                MachineFrameDecodeFailure::Uncorrelated { detail } => CentralFrame::Closed {
                    reason: SessionCloseReason::MalformedFrame { detail: *detail },
                },
            };
            Ok(vec![send(connection, frame)])
        }
        #[cfg(feature = "conformance")]
        SessionInput::ConformanceClose { call_id, reason } => {
            let mut effects = Vec::new();
            if let Some(session) = load_session(tx)? {
                effects.push(close(&session.connection, reason.clone()));
                effects.extend(disconnect(tx, &session, ids)?);
            }
            effects.push(SessionEffect::Return {
                call_id: call_id.clone(),
                result: SessionResult::ConformanceClosed,
            });
            Ok(effects)
        }
        SessionInput::Hello {
            connection,
            credential,
            installation,
            session_id,
            schema_support,
        } => {
            let expires_at = match credential {
                CredentialStatus::Rejected(reason) => {
                    return Ok(vec![close(connection, reason.clone())]);
                }
                CredentialStatus::Machine {
                    installation: verified,
                    expires_at,
                } if verified == installation => *expires_at,
                _ => return Ok(vec![close(connection, CredentialRejection::NotMachine)]),
            };
            if expires_at <= now.0 {
                return Ok(vec![close(connection, CredentialRejection::Expired)]);
            }
            let previous = load_session(tx)?;
            let session = SessionRecord {
                installation: installation.clone(),
                session_id: session_id.clone(),
                connection: connection.clone(),
                expires_at,
            };
            save_session(tx, &session)?;
            let mut effects = Vec::new();
            if let Some(old) = previous
                && old.connection != *connection
            {
                effects.push(send(
                    &old.connection,
                    CentralFrame::Closed {
                        reason: SessionCloseReason::ConnectionReplaced,
                    },
                ));
            }
            effects.push(session_dispatch::<RegistryKind>(
                tx,
                ids,
                (),
                RegistryCall::RegisterSession(SessionRegistration {
                    installation: installation.clone(),
                    session_id: session_id.clone(),
                    connection: connection.clone(),
                    schema_support: *schema_support,
                }),
                PendingContinuation::SessionRegister {
                    connection: connection.clone(),
                    session_id: session_id.clone(),
                },
            )?);
            if let Some(at) = crate::forward_rules::next_alarm(tx)? {
                effects.push(SessionEffect::SetAlarm { at });
            }
            Ok(effects)
        }
        SessionInput::Frame {
            connection,
            frame_id,
            request,
        } => {
            let Some(session) = load_session(tx)? else {
                return Ok(Vec::new());
            };
            if session.connection != *connection {
                return Ok(Vec::new());
            }
            if session.expires_at <= now.0 {
                let mut effects = disconnect(tx, &session, ids)?;
                effects.insert(0, close(connection, CredentialRejection::Expired));
                return Ok(effects);
            }
            if let SessionRequest::TailReport {
                project,
                epoch,
                request_ids,
                digest,
            } = request
            {
                return crate::session_tail_report::begin(
                    tx,
                    &session,
                    crate::session_tail_report::PendingTailReport {
                        connection: connection.clone(),
                        frame_id: frame_id.clone(),
                        report: TailReportRow {
                            project: project.clone(),
                            epoch: *epoch,
                            request_ids: request_ids.clone(),
                            digest: digest.clone(),
                        },
                    },
                    ids,
                );
            }
            if let SessionRequest::Read {
                scope: ReadScope::Notices,
                after,
            } = request
            {
                let body = crate::session_notices::read(tx, after.as_ref(), now)?;
                return Ok(vec![send(
                    connection,
                    CentralFrame::Answer {
                        frame_id: frame_id.clone(),
                        body,
                    },
                )]);
            }
            if let SessionRequest::Forward { project, post } = request {
                return crate::forward_rules::start(
                    tx, &session, frame_id, project, post, now, ids,
                );
            }
            let route = route(&session.installation, request)?;
            let expected = SessionReplyKind::of_call(&route)?;
            Ok(vec![route.persist(
                tx,
                ids,
                PendingContinuation::FrameAnswer {
                    connection: connection.clone(),
                    frame_id: frame_id.clone(),
                    expected,
                },
            )?])
        }
        SessionInput::ForwardAnswer {
            connection,
            forward_id,
            reply,
        } => {
            let Some(session) = load_session(tx)? else {
                return Ok(Vec::new());
            };
            if session.connection != *connection {
                return Ok(Vec::new());
            }
            if session.expires_at <= now.0 {
                let mut effects = disconnect(tx, &session, ids)?;
                effects.insert(0, close(connection, CredentialRejection::Expired));
                return Ok(effects);
            }
            crate::forward_rules::answer(tx, connection, forward_id, reply, now, ids)
        }
        SessionInput::Disconnected { connection } => {
            let Some(s) = load_session(tx)? else {
                return Ok(Vec::new());
            };
            if s.connection != *connection {
                return Ok(Vec::new());
            }
            disconnect(tx, &s, ids)
        }
        SessionInput::Alarm => {
            let mut effects = crate::forward_rules::alarm(tx, now)?;
            if let Some(s) = load_session(tx)?
                && s.expires_at <= now.0
            {
                effects.push(close(&s.connection, CredentialRejection::Expired));
                effects.extend(disconnect(tx, &s, ids)?);
            }
            if let Some(at) = crate::forward_rules::next_alarm(tx)? {
                effects.push(SessionEffect::SetAlarm { at });
            }
            Ok(effects)
        }
        _ => Err(CentralError::InvalidInput {
            message: "not a session input".into(),
        }),
    }
}
fn disconnect(
    tx: &mut dyn SqlTx,
    session: &SessionRecord,
    ids: &mut IdBatch,
) -> Result<Vec<SessionEffect>, CentralError> {
    tx.execute(statement(
        "DELETE FROM _session_state WHERE singleton=1",
        Vec::new(),
    ))?;
    Ok(vec![SessionEffect::Dispatch(SessionDispatch::Registry(
        Dispatch {
            carry: None,
            call_id: ids
                .call_ids
                .pop()
                .ok_or_else(|| CentralError::InvalidInput {
                    message: "missing disconnect ID".into(),
                })?,
            to: (),
            call: RegistryCall::SessionDisconnected {
                installation: session.installation.clone(),
                connection: session.connection.clone(),
            },
        },
    ))])
}
pub(crate) enum SessionRoute {
    Registry(RegistryCall),
    Copy { project: ProjectId, call: CopyCall },
    Fleet(FleetCall),
}
impl SessionRoute {
    fn persist(
        self,
        tx: &mut dyn SqlTx,
        ids: &mut IdBatch,
        pending: PendingContinuation,
    ) -> Result<SessionEffect, CentralError> {
        match self {
            Self::Registry(call) => session_dispatch::<RegistryKind>(tx, ids, (), call, pending),
            Self::Copy { project, call } => {
                session_dispatch::<CopyKind>(tx, ids, project, call, pending)
            }
            Self::Fleet(call) => session_dispatch::<FleetKind>(tx, ids, (), call, pending),
        }
    }
}
fn route(
    installation: &InstallationId,
    request: &SessionRequest,
) -> Result<SessionRoute, CentralError> {
    Ok(match request {
        SessionRequest::Claim { attempt, project } => SessionRoute::Registry(RegistryCall::Claim {
            attempt: attempt.clone(),
            project: project.clone(),
            claimant: installation.clone(),
        }),
        SessionRequest::Forward { .. } => return Err(CentralError::ReplyContract),
        SessionRequest::Piece(piece) => SessionRoute::Copy {
            project: piece.project.clone(),
            call: CopyCall::ApplyPiece {
                piece: piece.clone(),
                sender: installation.clone(),
            },
        },
        SessionRequest::Renew {
            attempt,
            project,
            epoch,
        } => SessionRoute::Registry(RegistryCall::Renew {
            installation: installation.clone(),
            attempt: attempt.clone(),
            project: project.clone(),
            epoch: *epoch,
        }),
        SessionRequest::Release {
            project,
            epoch,
            transfer,
            outcome,
        } => SessionRoute::Registry(RegistryCall::Release {
            installation: installation.clone(),
            project: project.clone(),
            epoch: *epoch,
            transfer: transfer.clone(),
            outcome: outcome.clone(),
        }),
        SessionRequest::ReserveFirstEpoch { project } => {
            SessionRoute::Registry(RegistryCall::ReserveFirstEpoch {
                project: project.clone(),
                source: ReservationSource::Machine {
                    installation: installation.clone(),
                },
            })
        }
        SessionRequest::TailReport { .. } => return Err(CentralError::ReplyContract),
        SessionRequest::Fleet(request) => SessionRoute::Fleet(FleetCall::Fleet {
            installation: installation.clone(),
            request: request.clone(),
        }),
        SessionRequest::Read {
            scope: ReadScope::ProjectCopy { project },
            after,
        } => {
            let cursor = match after {
                Some(ReadCursor::Snapshot(cursor)) => Some(cursor.clone()),
                None => None,
                _ => {
                    return Err(CentralError::InvalidInput {
                        message: "wrong read cursor".into(),
                    });
                }
            };
            SessionRoute::Copy {
                project: project.clone(),
                call: CopyCall::SnapshotPage { cursor },
            }
        }
        SessionRequest::Read { scope, after } => SessionRoute::Fleet(FleetCall::Read {
            installation: installation.clone(),
            scope: scope.clone(),
            after: after.clone(),
        }),
        SessionRequest::ArchiveSeen {
            board,
            project,
            position,
        } => SessionRoute::Fleet(FleetCall::ArchiveSeen {
            installation: installation.clone(),
            board: board.clone(),
            project: project.clone(),
            position: *position,
        }),
    })
}
pub(crate) fn continuation(
    tx: &mut dyn SqlTx,
    pending: PendingContinuation,
    result: SessionReply,
    now: RegistryTime,
    ids: &mut IdBatch,
) -> Result<Vec<SessionEffect>, CentralError> {
    match (pending, result) {
        (
            PendingContinuation::TailVerification(pending),
            SessionReply::Registry(RegistryResult::TailVerification(result)),
        ) => crate::session_tail_report::verified(tx, pending, result, now, ids),
        (
            PendingContinuation::SessionRegister {
                connection,
                session_id,
            },
            SessionReply::Registry(RegistryResult::Registered(registration)),
        ) => {
            if !current(tx, &connection, now)? {
                return Ok(Vec::new());
            }
            match registration {
                RegistrationResult::Rejected(reason) => {
                    let s = load_session(tx)?.ok_or(CentralError::ReplyContract)?;
                    let mut effects = disconnect(tx, &s, ids)?;
                    effects.insert(0, close(&connection, reason));
                    Ok(effects)
                }
                RegistrationResult::Registered { projects } => {
                    welcome(tx, connection, session_id, projects, Vec::new(), ids)
                }
            }
        }
        (
            PendingContinuation::WelcomeCopy {
                connection,
                session_id,
                lease,
                remaining,
                mut complete,
            },
            SessionReply::Copy(CopyResult::CopyStatus(status)),
        ) => {
            if !current(tx, &connection, now)? {
                return Ok(Vec::new());
            }
            let s = status.ok_or_else(|| StoreError::Corrupt {
                field: "welcome initialized copy".into(),
            })?;
            complete.push(ProjectSessionState {
                project: lease.project.clone(),
                acked: s.acked,
                lease,
            });
            welcome(tx, connection, session_id, remaining, complete, ids)
        }
        (
            PendingContinuation::FleetReservation {
                connection,
                frame_id,
                answer,
            },
            SessionReply::Registry(RegistryResult::FirstLease(reservation)),
        ) => {
            if !current(tx, &connection, now)? {
                return Ok(Vec::new());
            }
            Ok(vec![send(
                &connection,
                CentralFrame::Answer {
                    frame_id,
                    body: if matches!(reservation, SessionAnswer::Unavailable { .. }) {
                        reservation
                    } else {
                        answer
                    },
                },
            )])
        }
        (
            PendingContinuation::FrameAnswer {
                connection,
                frame_id,
                expected,
            },
            result,
        ) => {
            if !current(tx, &connection, now)? {
                return Ok(Vec::new());
            }
            if let SessionReply::Fleet(FleetResult::ProjectAccepted { project, answer }) = result {
                if expected != SessionReplyKind::Fleet {
                    return Err(CentralError::ReplyContract);
                }
                let session = load_session(tx)?.ok_or(CentralError::ReplyContract)?;
                return Ok(vec![session_dispatch::<RegistryKind>(
                    tx,
                    ids,
                    (),
                    RegistryCall::ReserveFirstEpoch {
                        project,
                        source: ReservationSource::FleetAcceptance {
                            installation: session.installation,
                        },
                    },
                    PendingContinuation::FleetReservation {
                        connection,
                        frame_id,
                        answer,
                    },
                )?]);
            }
            let body = expected.answer(result)?;
            Ok(vec![send(
                &connection,
                CentralFrame::Answer { frame_id, body },
            )])
        }
        _ => Err(CentralError::ReplyContract),
    }
}
fn welcome(
    tx: &mut dyn SqlTx,
    connection: ConnectionId,
    session_id: SessionId,
    mut remaining: Vec<LeaseStatus>,
    complete: Vec<ProjectSessionState>,
    ids: &mut IdBatch,
) -> Result<Vec<SessionEffect>, CentralError> {
    if let Some(lease) = remaining.pop() {
        Ok(vec![session_dispatch::<CopyKind>(
            tx,
            ids,
            lease.project.clone(),
            CopyCall::CopyStatus {
                project: lease.project.clone(),
            },
            PendingContinuation::WelcomeCopy {
                connection,
                session_id,
                lease,
                remaining,
                complete,
            },
        )?])
    } else {
        Ok(vec![send(
            &connection,
            CentralFrame::Welcome {
                session_id,
                projects: complete,
            },
        )])
    }
}
