use crate::copy_storage::{encode_record, query_statement, statement, text};
use crate::forward_contract::{DeliveredForward, ForwardPhase, PendingForward};
use crate::registry_storage::{load_lease, load_machine, read_record};
use crate::*;
use collaboration_replication_protocol::*;

pub(crate) fn route(
    tx: &mut dyn SqlTx,
    project: &ProjectId,
    sender: &InstallationId,
) -> Result<ForwardRouteResult, CentralError> {
    let Some(lease) = load_lease(tx, project)? else {
        return Ok(ForwardRouteResult::NotHeld);
    };
    if lease.holder == *sender {
        return Ok(ForwardRouteResult::SenderHolds);
    }
    let origin = load_machine(tx, sender)?.ok_or_else(|| StoreError::Corrupt {
        field: "forward sender enrollment".into(),
    })?;
    let holder = load_machine(tx, &lease.holder)?.ok_or_else(|| StoreError::Corrupt {
        field: "forward holder enrollment".into(),
    })?;
    Ok(ForwardRouteResult::Route {
        holder: lease.holder,
        sender_label: origin.label,
        holder_last_seen: holder.last_seen,
    })
}
fn new_call_id(ids: &mut IdBatch) -> Result<ObjectCallId, CentralError> {
    ids.call_ids
        .pop()
        .ok_or_else(|| CentralError::InvalidInput {
            message: "missing forwarding call ID".into(),
        })
}
pub(crate) fn load_pending(
    tx: &mut dyn SqlTx,
    key: &str,
    call: bool,
) -> Result<Option<PendingForward>, CentralError> {
    read_record(
        tx,
        if call {
            "SELECT record FROM pending_forwards WHERE call_id=?"
        } else {
            "SELECT record FROM pending_forwards WHERE forward_id=?"
        },
        key,
    )
}
fn save_pending(tx: &mut dyn SqlTx, pending: &PendingForward) -> Result<(), CentralError> {
    tx.execute(statement("INSERT INTO pending_forwards(forward_id,call_id,record) VALUES(?,?,?) ON CONFLICT(forward_id) DO UPDATE SET call_id=excluded.call_id,record=excluded.record",vec![text(pending.forward_id.as_str()),text(pending.call_id.as_str()),text(encode_record(pending)?)]))?;
    Ok(())
}
pub(crate) fn terminal_write(
    tx: &mut dyn SqlTx,
    pending: &PendingForward,
    outcome: ForwardOutcome,
    now: RegistryTime,
) -> Result<Option<SocketWrite>, CentralError> {
    tx.execute(statement(
        "DELETE FROM pending_forwards WHERE forward_id=?",
        vec![text(pending.forward_id.as_str())],
    ))?;
    if crate::session_rules::load_session(tx)?.is_some_and(|session| {
        session.connection != pending.connection_id || session.expires_at <= now.0
    }) {
        return Ok(None);
    }
    Ok(Some(SocketWrite {
        to: pending.connection_id.clone(),
        frame: CentralFrame::Answer {
            frame_id: pending.frame_id.clone(),
            body: SessionAnswer::Forwarded(outcome),
        },
    }))
}
fn terminal(
    tx: &mut dyn SqlTx,
    pending: &PendingForward,
    outcome: ForwardOutcome,
    now: RegistryTime,
) -> Result<Vec<SessionEffect>, CentralError> {
    Ok(terminal_write(tx, pending, outcome, now)?
        .into_iter()
        .map(|write| SessionEffect::Send {
            to: write.to,
            frame: write.frame,
        })
        .collect())
}
pub(crate) fn start(
    tx: &mut dyn SqlTx,
    session: &crate::session_rules::SessionRecord,
    frame: &FrameId,
    project: &ProjectId,
    post: &ForwardedPost,
    now: RegistryTime,
    ids: &mut IdBatch,
) -> Result<Vec<SessionEffect>, CentralError> {
    let count = tx
        .query(query_statement(
            "SELECT COUNT(*) AS total FROM pending_forwards",
            Vec::new(),
            ["total"],
        ))?
        .first()
        .ok_or_else(|| StoreError::Corrupt {
            field: "forward count".into(),
        })?
        .integer("total")?;
    if count >= i64::try_from(FORWARD_IN_FLIGHT_LIMIT).map_err(|_| CentralError::ReplyContract)? {
        return Ok(vec![SessionEffect::Send {
            to: session.connection.clone(),
            frame: CentralFrame::Answer {
                frame_id: frame.clone(),
                body: SessionAnswer::Forwarded(ForwardOutcome::Overloaded),
            },
        }]);
    }
    let call_id = new_call_id(ids)?;
    let pending = PendingForward {
        forward_id: ForwardId::new(call_id.as_str()).map_err(|error| {
            CentralError::InvalidInput {
                message: error.to_string(),
            }
        })?,
        call_id: call_id.clone(),
        origin: session.installation.clone(),
        connection_id: session.connection.clone(),
        frame_id: frame.clone(),
        project: project.clone(),
        phase: ForwardPhase::Routing,
        deadline: now.0 + chrono::TimeDelta::seconds(FORWARD_DEADLINE_SECONDS),
    };
    save_pending(tx, &pending)?;
    let mut effects = Vec::new();
    if let Some(at) = next_alarm(tx)? {
        effects.push(SessionEffect::SetAlarm { at });
    }
    effects.push(SessionEffect::Dispatch(SessionDispatch::Registry(
        Dispatch {
            call_id,
            to: (),
            call: RegistryCall::ForwardRoute {
                project: project.clone(),
                sender: session.installation.clone(),
            },
            carry: Some(DispatchCarry::ForwardPost { post: post.clone() }),
        },
    )));
    Ok(effects)
}
pub(crate) fn result(
    tx: &mut dyn SqlTx,
    id: &ObjectCallId,
    result: &SessionReply,
    carry: Option<DispatchCarry>,
    now: RegistryTime,
    ids: &mut IdBatch,
) -> Result<Vec<SessionEffect>, CentralError> {
    let Some(mut pending) = load_pending(tx, id.as_str(), true)? else {
        return Ok(Vec::new());
    };
    if pending.deadline <= now.0 {
        return Ok(Vec::new());
    }
    match (&pending.phase, result, carry) {
        (
            ForwardPhase::Routing,
            SessionReply::Registry(RegistryResult::ForwardRoute(ForwardRouteResult::SenderHolds)),
            _,
        ) => terminal(tx, &pending, ForwardOutcome::SenderHolds, now),
        (
            ForwardPhase::Routing,
            SessionReply::Registry(RegistryResult::ForwardRoute(ForwardRouteResult::NotHeld)),
            _,
        ) => terminal(tx, &pending, ForwardOutcome::NotHeld, now),
        (
            ForwardPhase::Routing,
            SessionReply::Registry(RegistryResult::ForwardRoute(ForwardRouteResult::Route {
                holder,
                sender_label,
                holder_last_seen,
            })),
            Some(DispatchCarry::ForwardPost { post }),
        ) => {
            pending.phase = ForwardPhase::AwaitingHolder {
                expected_holder: holder.clone(),
            };
            pending.call_id = new_call_id(ids)?;
            save_pending(tx, &pending)?;
            Ok(vec![SessionEffect::Dispatch(SessionDispatch::Delivery(
                Dispatch {
                    call_id: pending.call_id,
                    to: holder.clone(),
                    call: DeliveryCall::DeliverForward {
                        forward_id: pending.forward_id,
                        origin: pending.origin,
                        origin_label: sender_label.clone(),
                        project: pending.project,
                        post,
                        deadline: pending.deadline,
                    },
                    carry: Some(DispatchCarry::ForwardDelivery {
                        holder_last_seen: *holder_last_seen,
                    }),
                },
            ))])
        }
        (
            ForwardPhase::AwaitingHolder { .. },
            SessionReply::Delivery(DeliveryResult::DeliverForward(
                ForwardDeliveryResult::NoConnection,
            )),
            Some(DispatchCarry::ForwardDelivery { holder_last_seen }),
        ) => terminal(
            tx,
            &pending,
            ForwardOutcome::HolderUnreachable {
                last_seen: holder_last_seen,
            },
            now,
        ),
        (
            ForwardPhase::AwaitingHolder { .. },
            SessionReply::Delivery(DeliveryResult::DeliverForward(
                ForwardDeliveryResult::Delivered,
            )),
            _,
        ) => Ok(Vec::new()),
        _ => Err(CentralError::ReplyContract),
    }
}
pub(crate) fn failed(
    tx: &mut dyn SqlTx,
    id: &ObjectCallId,
    now: RegistryTime,
) -> Result<Vec<SessionEffect>, CentralError> {
    let Some(pending) = load_pending(tx, id.as_str(), true)? else {
        return Ok(Vec::new());
    };
    if pending.deadline <= now.0 {
        return Ok(Vec::new());
    }
    terminal(tx, &pending, ForwardOutcome::Unanswered, now)
}
pub(crate) fn answer(
    tx: &mut dyn SqlTx,
    connection: &ConnectionId,
    id: &ForwardId,
    reply: &BoardPayload,
    now: RegistryTime,
    ids: &mut IdBatch,
) -> Result<Vec<SessionEffect>, CentralError> {
    if !crate::session_rules::current(tx, connection, now)? {
        return Ok(Vec::new());
    }
    let delivered: Option<DeliveredForward> = read_record(
        tx,
        "SELECT record FROM delivered_forwards WHERE forward_id=?",
        id.as_str(),
    )?;
    let Some(delivered) = delivered else {
        return Ok(Vec::new());
    };
    if delivered.deadline <= now.0 {
        return Ok(Vec::new());
    }
    tx.execute(statement(
        "DELETE FROM delivered_forwards WHERE forward_id=?",
        vec![text(id.as_str())],
    ))?;
    Ok(vec![SessionEffect::Dispatch(SessionDispatch::Delivery(
        Dispatch {
            call_id: new_call_id(ids)?,
            to: delivered.origin,
            call: DeliveryCall::ForwardAnswered {
                forward_id: id.clone(),
                project: delivered.project,
                outcome: ForwardOutcome::Answered {
                    reply: reply.clone(),
                },
            },
            carry: None,
        },
    ))])
}
pub(crate) fn alarm(
    tx: &mut dyn SqlTx,
    now: RegistryTime,
) -> Result<Vec<SessionEffect>, CentralError> {
    let mut effects = Vec::new();
    for row in tx.query(query_statement(
        "SELECT record FROM pending_forwards",
        Vec::new(),
        ["record"],
    ))? {
        let pending: PendingForward =
            crate::copy_storage::decode_record(row.text("record")?, "pending forward")?;
        if pending.deadline <= now.0 {
            effects.extend(terminal(tx, &pending, ForwardOutcome::Unanswered, now)?);
        }
    }
    Ok(effects)
}
pub(crate) fn next_alarm(tx: &mut dyn SqlTx) -> Result<Option<RegistryTime>, CentralError> {
    let mut deadline = crate::session_rules::load_session(tx)?.map(|session| session.expires_at);
    for row in tx.query(query_statement(
        "SELECT record FROM pending_forwards",
        Vec::new(),
        ["record"],
    ))? {
        let pending: PendingForward =
            crate::copy_storage::decode_record(row.text("record")?, "pending forward")?;
        deadline = Some(deadline.map_or(pending.deadline, |at| at.min(pending.deadline)));
    }
    Ok(deadline.map(RegistryTime))
}
