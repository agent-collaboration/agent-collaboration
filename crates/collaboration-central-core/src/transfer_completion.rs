use crate::copy_storage::{encode_record, statement, text, unsigned};
use crate::registry_contract::{StoredPreview, TransferReply};
use crate::registry_rules::returned;
use crate::registry_storage::*;
use crate::transfer_rules::{fence_dispatch, notify, refuse};
use crate::*;
use crate::{
    AdminRefusal, AdminResult, CentralError, FenceAttempt, FenceExpectation, FenceResult, IdBatch,
    ObjectCallId, RegistryLease, RegistryTime, SqlTx, TransferRecord, TransferState,
};
use chrono::TimeDelta;
use collaboration_replication_protocol::{
    Epoch, NoticeKind, SessionAnswer, SessionRefusal, TakeoverPreview, TransferId,
};
pub(crate) fn preview_result(
    tx: &mut dyn SqlTx,
    id: &ObjectCallId,
    lease: RegistryLease,
    status: Option<crate::CopyStatus>,
    now: RegistryTime,
) -> Result<Vec<RegistryEffect>, CentralError> {
    let Some(s) = status else {
        return Ok(refuse(id, AdminRefusal::RegistryEpochUnproven));
    };
    let last_seen = load_machine(tx, &lease.holder)?.map_or(lease.last_renewed_at, |m| m.last_seen);
    let p = StoredPreview {
        project: lease.project.clone(),
        holder: lease.holder.clone(),
        epoch: lease.epoch,
        copy_fence: s.fence,
        acked: s.acked,
        schema: s.schema,
        last_seen,
    };
    tx.execute(statement("INSERT INTO _registry_previews(token,record) VALUES(?,?) ON CONFLICT(token) DO UPDATE SET record=excluded.record",vec![text(id.as_str()),text(encode_record(&p)?)]))?;
    Ok(returned(
        id,
        RegistryResult::Admin(AdminResult::Preview(TakeoverPreview {
            holder: lease.holder,
            epoch: lease.epoch,
            acked: p.acked,
            last_seen,
            tail_estimate_ms: (now.0 - last_seen).num_milliseconds().max(0).unsigned_abs(),
            confirm_token: id.as_str().into(),
        })),
    ))
}
pub(crate) fn fence_result(
    tx: &mut dyn SqlTx,
    reply: TransferReply,
    id: &TransferId,
    result: FenceResult,
    now: RegistryTime,
    ids: &mut IdBatch,
) -> Result<Vec<RegistryEffect>, CentralError> {
    if let TransferReply::Claim(reply_id) = &reply {
        return crate::claim_rules::fence_result(tx, reply_id, id, result, now, ids);
    }
    let mut t = load_transfer(tx, id)?.ok_or_else(|| crate::StoreError::Corrupt {
        field: "pending transfer".into(),
    })?;
    if matches!(
        t.state,
        TransferState::Granted { .. } | TransferState::Refused(_)
    ) {
        return reply_transfer(&reply, &t);
    }
    if let (TransferReply::Release(id), FenceResult::PreviewStale { acked }) = (&reply, &result) {
        t.state = TransferState::Draining;
        save_transfer(tx, &t)?;
        return Ok(returned(
            id,
            RegistryResult::Release(SessionAnswer::Refused(SessionRefusal::PositionGap {
                acked: *acked,
            })),
        ));
    }
    let mut notices = Vec::new();
    t.state = match result {
        FenceResult::Fenced {
            new_epoch, start, ..
        } => {
            let old = load_lease(tx, &t.project)?.ok_or_else(|| crate::StoreError::Corrupt {
                field: "transfer lease".into(),
            })?;
            tx.execute(statement(
                "UPDATE _registry_history SET cutoff=? WHERE project=? AND epoch=?",
                vec![
                    text(encode_record(&start)?),
                    text(t.project.as_str()),
                    unsigned(t.old_epoch.value()),
                ],
            ))?;
            save_lease(
                tx,
                &RegistryLease {
                    change: std::num::NonZeroU64::MIN,
                    project: t.project.clone(),
                    holder: t.to_installation.clone(),
                    epoch: new_epoch,
                    expires_at: now.0 + TimeDelta::hours(4),
                    last_renewed_at: now.0,
                },
            )?;
            notices.push(notify(
                &old.holder,
                NoticeKind::LeaseSuperseded {
                    project: t.project.clone(),
                    epoch: t.old_epoch,
                },
            ));
            notices.push(notify(
                &t.to_installation,
                NoticeKind::LeaseGranted {
                    project: t.project.clone(),
                    epoch: new_epoch,
                },
            ));
            TransferState::Granted { new_epoch, start }
        }
        FenceResult::PreviewStale { acked } => {
            TransferState::Refused(AdminRefusal::PreviewStale { acked })
        }
        FenceResult::SchemaTooNew { store, supported } => {
            TransferState::Refused(AdminRefusal::SchemaTooNew {
                store,
                supported: Some(supported),
            })
        }
        FenceResult::NotInitialized => TransferState::Refused(AdminRefusal::RegistryEpochUnproven),
        FenceResult::FenceNotAdvanced { current_epoch } => {
            if let TransferState::Fencing {
                expect,
                target_schema_max,
                attempt: FenceAttempt::Initial,
                ..
            } = &t.state
                && let Some(next) = current_epoch
                    .value()
                    .checked_add(1)
                    .and_then(|v| Epoch::new(v).ok())
            {
                t.state = TransferState::Fencing {
                    new_epoch: next,
                    expect: FenceExpectation {
                        epoch: current_epoch,
                        acked: expect.acked,
                    },
                    target_schema_max: *target_schema_max,
                    attempt: FenceAttempt::FloorRepair,
                };
                save_transfer(tx, &t)?;
                return fence_dispatch(tx, reply, &t, ids);
            }
            TransferState::Refused(AdminRefusal::RegistryEpochUnproven)
        }
    };
    save_transfer(tx, &t)?;
    let mut effects = reply_transfer(&reply, &t)?;
    effects.extend(notices);
    if let TransferState::Granted { .. } = &t.state {
        let lease = load_lease(tx, &t.project)?.ok_or(CentralError::ReplyContract)?;
        effects.extend(crate::lease_publication::changed(
            tx,
            &lease.project,
            now,
            ids,
        )?);
    }
    Ok(effects)
}
pub(crate) fn reply_transfer(
    reply: &TransferReply,
    t: &TransferRecord,
) -> Result<Vec<RegistryEffect>, CentralError> {
    match reply {
        TransferReply::Claim(id) => crate::claim_rules::reply(id, t),
        TransferReply::Admin(id) => Ok(returned(
            id,
            RegistryResult::Admin(match &t.state {
                TransferState::Granted { new_epoch, start } => AdminResult::Granted {
                    new_epoch: *new_epoch,
                    start: *start,
                },
                TransferState::Refused(r) => AdminResult::Refused(r.clone()),
                _ => return Err(CentralError::ReplyContract),
            }),
        )),
        TransferReply::Release(id) => Ok(returned(
            id,
            RegistryResult::Release(match &t.state {
                TransferState::Granted { start, .. } => SessionAnswer::Released {
                    final_position: *start,
                },
                TransferState::Refused(AdminRefusal::PreviewStale { acked }) => {
                    SessionAnswer::Refused(SessionRefusal::PositionGap { acked: *acked })
                }
                TransferState::Refused(AdminRefusal::MigrationPending(r)) => {
                    SessionAnswer::ReleaseRefused {
                        transfer: t.transfer_id.clone(),
                        reason: r.clone(),
                    }
                }
                TransferState::Refused(AdminRefusal::SchemaTooNew { store, supported }) => {
                    SessionAnswer::ReleaseRefused {
                        transfer: t.transfer_id.clone(),
                        reason: collaboration_replication_protocol::ReleaseRefusal::SchemaTooNew {
                            store: store.clone(),
                            supported: *supported,
                        },
                    }
                }
                TransferState::Refused(_) => SessionAnswer::Refused(SessionRefusal::NotWriter {
                    current: Some(t.old_epoch),
                }),
                _ => return Err(CentralError::ReplyContract),
            }),
        )),
    }
}
