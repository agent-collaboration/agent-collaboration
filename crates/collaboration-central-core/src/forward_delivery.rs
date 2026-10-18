use crate::copy_storage::{decode_record, encode_record, query_statement, statement, text};
use crate::forward_contract::{DeliveredForward, ForwardPhase};
use crate::forward_rules::{load_pending, terminal_write};
use crate::*;
use collaboration_replication_protocol::*;
pub(crate) fn call(
    tx: &mut dyn SqlTx,
    from: &ObjectAddress,
    id: &ObjectCallId,
    call: &DeliveryCall,
    now: RegistryTime,
) -> Result<Vec<DeliveryEffect>, CentralError> {
    match call {
        DeliveryCall::DeliverForward {
            forward_id,
            origin,
            origin_label,
            project,
            post,
            deadline,
        } => {
            if from != &ObjectAddress::Connection(origin.clone()) {
                return Err(CentralError::InvalidInput {
                    message: "forward origin differs from caller".into(),
                });
            }
            let Some(session) = crate::session_rules::load_session(tx)?
                .filter(|session| session.expires_at > now.0)
            else {
                return Ok(crate::delivery_rules::returned(
                    id,
                    DeliveryResult::DeliverForward(ForwardDeliveryResult::NoConnection),
                ));
            };
            if *deadline <= now.0 {
                return Ok(crate::delivery_rules::returned(
                    id,
                    DeliveryResult::DeliverForward(ForwardDeliveryResult::Delivered),
                ));
            }
            let delivered = DeliveredForward {
                forward_id: forward_id.clone(),
                origin: origin.clone(),
                project: project.clone(),
                deadline: *deadline,
            };
            tx.execute(statement("INSERT INTO delivered_forwards(forward_id,record) VALUES(?,?) ON CONFLICT(forward_id) DO NOTHING",vec![text(forward_id.as_str()),text(encode_record(&delivered)?)]))?;
            let mut effects = crate::delivery_rules::returned(
                id,
                DeliveryResult::DeliverForward(ForwardDeliveryResult::Delivered),
            );
            effects.push(DeliveryEffect::Send {
                to: session.connection,
                frame: CentralFrame::Forwarded {
                    forward_id: forward_id.clone(),
                    origin: origin.clone(),
                    origin_label: origin_label.clone(),
                    project: project.clone(),
                    post: post.clone(),
                },
            });
            if let Some(at) = next_alarm(tx)? {
                effects.push(DeliveryEffect::SetAlarm { at });
            }
            Ok(effects)
        }
        DeliveryCall::ForwardAnswered {
            forward_id,
            project,
            outcome,
        } => {
            let mut effects = crate::delivery_rules::returned(id, DeliveryResult::ForwardAck);
            if let Some(pending) = load_pending(tx, forward_id.as_str(), false)?
                && pending.deadline > now.0
                && pending.project == *project
                && let ForwardPhase::AwaitingHolder { expected_holder } = &pending.phase
                && from == &ObjectAddress::Connection(expected_holder.clone())
                && matches!(outcome, ForwardOutcome::Answered { .. })
            {
                effects.extend(
                    terminal_write(tx, &pending, outcome.clone(), now)?
                        .into_iter()
                        .map(|write| DeliveryEffect::Send {
                            to: write.to,
                            frame: write.frame,
                        }),
                );
            }
            Ok(effects)
        }
        _ => Err(CentralError::ReplyContract),
    }
}

pub(crate) fn alarm(
    tx: &mut dyn SqlTx,
    now: RegistryTime,
) -> Result<Vec<DeliveryEffect>, CentralError> {
    for row in tx.query(query_statement(
        "SELECT record FROM delivered_forwards",
        Vec::new(),
        ["record"],
    ))? {
        let delivered: DeliveredForward = decode_record(row.text("record")?, "delivered forward")?;
        if delivered.deadline <= now.0 {
            tx.execute(statement(
                "DELETE FROM delivered_forwards WHERE forward_id=?",
                vec![text(delivered.forward_id.as_str())],
            ))?;
        }
    }
    Ok(next_alarm(tx)?
        .into_iter()
        .map(|at| DeliveryEffect::SetAlarm { at })
        .collect())
}
pub(crate) fn next_alarm(tx: &mut dyn SqlTx) -> Result<Option<RegistryTime>, CentralError> {
    let mut deadline = None;
    for row in tx.query(query_statement(
        "SELECT record FROM delivered_forwards",
        Vec::new(),
        ["record"],
    ))? {
        let delivered: DeliveredForward = decode_record(row.text("record")?, "delivered forward")?;
        deadline = Some(
            deadline.map_or(delivered.deadline, |at: chrono::DateTime<chrono::Utc>| {
                at.min(delivered.deadline)
            }),
        );
    }
    Ok(deadline.map(RegistryTime))
}
