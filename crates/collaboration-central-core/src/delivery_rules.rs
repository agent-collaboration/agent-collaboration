use crate::copy_storage::statement;
use crate::session_rules::{bootstrap, load_session};
use crate::*;
use collaboration_replication_protocol::*;
pub(crate) fn returned(id: &ObjectCallId, result: DeliveryResult) -> Vec<DeliveryEffect> {
    vec![DeliveryEffect::Return {
        call_id: id.clone(),
        result,
    }]
}
pub(crate) fn call(
    tx: &mut dyn SqlTx,
    id: &ObjectCallId,
    call: &DeliveryCall,
    now: RegistryTime,
) -> Result<Vec<DeliveryEffect>, CentralError> {
    bootstrap(tx)?;
    match call {
        DeliveryCall::CloseSession { reason } => {
            let mut effects = returned(id, DeliveryResult::SessionClosed);
            if let Some(s) = load_session(tx)? {
                effects.push(DeliveryEffect::Send {
                    to: s.connection,
                    frame: CentralFrame::Closed {
                        reason: SessionCloseReason::CredentialRejected(reason.clone()),
                    },
                });
                tx.execute(statement(
                    "DELETE FROM _session_state WHERE singleton=1",
                    Vec::new(),
                ))?;
            }
            Ok(effects)
        }
        DeliveryCall::DeliverNotice(kind) => {
            let notice = crate::session_notices::append(tx, kind, now)?;
            let mut effects = returned(id, DeliveryResult::NoticeDelivered);
            if let Some(s) = load_session(tx)?
                && s.expires_at > now.0
            {
                effects.push(DeliveryEffect::Send {
                    to: s.connection,
                    frame: CentralFrame::Notice {
                        position: notice.position,
                        kind: notice.kind,
                    },
                });
            }
            Ok(effects)
        }
        _ => Err(CentralError::ReplyContract),
    }
}
