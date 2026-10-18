use crate::copy_storage::query_statement;
use crate::copy_storage::{encode_record, statement, text, unsigned};
use crate::registry_contract::{PendingContinuation, StoredPreview, TransferReply};
use crate::registry_rules::returned;
use crate::registry_storage::*;
use crate::*;
use crate::{
    AdminAuthority, AdminRefusal, AdminRequest, AdminResult, CentralError, FenceAttempt,
    FenceExpectation, IdBatch, ObjectCallId, RegistryTime, SqlTx, TransferKind, TransferRecord,
    TransferState,
};
use chrono::TimeDelta;
use collaboration_replication_protocol::{
    Epoch, NoticeKind, ReleaseOutcome, SessionAnswer, SessionRefusal,
};
use sha2::{Digest, Sha256};
pub(crate) fn refuse(id: &ObjectCallId, reason: AdminRefusal) -> Vec<RegistryEffect> {
    returned(id, RegistryResult::Admin(AdminResult::Refused(reason)))
}
pub(crate) fn admin(
    tx: &mut dyn SqlTx,
    id: &ObjectCallId,
    authority: &AdminAuthority,
    request: &AdminRequest,
    now: RegistryTime,
    ids: &mut IdBatch,
) -> Result<Vec<RegistryEffect>, CentralError> {
    let AdminAuthority::Admin { subject } = authority else {
        return Ok(refuse(id, AdminRefusal::AdminRequired));
    };
    match request {
        AdminRequest::ImportProject { project, upload } => Ok(vec![dispatch_copy(
            tx,
            ids,
            project.clone(),
            CopyCall::ImportSeed {
                project: project.clone(),
                upload: upload.clone(),
            },
            PendingContinuation::ImportProject {
                reply_id: id.clone(),
                project: project.clone(),
            },
        )?]),
        AdminRequest::EnrollMachine {
            label,
            installation,
            credential_hash,
            access_client_id,
            expires_at,
        } => {
            if label.trim().is_empty() || credential_hash.trim().is_empty() || *expires_at <= now.0
            {
                return Err(CentralError::InvalidInput {
                    message: "empty credential hash or expired enrollment".into(),
                });
            }
            for row in tx.query(query_statement(
                "SELECT record FROM _registry_machines",
                Vec::new(),
                ["record"],
            ))? {
                let enrolled: MachineRecord =
                    crate::copy_storage::decode_record(row.text("record")?, "machine")?;
                if enrolled.access_client_id == *access_client_id
                    && enrolled.installation != *installation
                {
                    return Err(CentralError::InvalidInput {
                        message: "Access client ID belongs to another installation".into(),
                    });
                }
            }
            let machine = MachineRecord {
                label: label.clone(),
                installation: installation.clone(),
                access_client_id: access_client_id.clone(),
                state: MachineState::Enrolled,
                expires_at: *expires_at,
                schema_support: None,
                last_seen: now.0,
                connection: None,
            };
            tx.execute(statement("INSERT INTO _registry_machines(installation,credential_hash,record) VALUES(?,?,?) ON CONFLICT(installation) DO UPDATE SET credential_hash=excluded.credential_hash,record=excluded.record",vec![text(installation.as_str()),text(credential_hash),text(encode_record(&machine)?)]))?;
            Ok(returned(
                id,
                RegistryResult::Admin(AdminResult::MachineEnrolled),
            ))
        }
        AdminRequest::RevokeMachine { installation } => {
            if let Some(mut m) = load_machine(tx, installation)? {
                m.state = MachineState::Revoked;
                m.connection = None;
                save_machine(tx, &m)?;
            }
            let mut effects = returned(id, RegistryResult::Admin(AdminResult::MachineRevoked));
            effects.push(RegistryEffect::Dispatch(RegistryDispatch::Delivery(
                Dispatch {
                    carry: None,
                    call_id: ids
                        .call_ids
                        .pop()
                        .ok_or_else(|| CentralError::InvalidInput {
                            message: "missing revoke call ID".into(),
                        })?,
                    to: installation.clone(),
                    call: DeliveryCall::CloseSession {
                        reason: collaboration_replication_protocol::CredentialRejection::Revoked,
                    },
                },
            )));
            Ok(effects)
        }
        AdminRequest::TakeoverPreview { project } => {
            let Some(lease) = load_lease(tx, project)? else {
                return Ok(refuse(id, AdminRefusal::NotInitialized));
            };
            Ok(vec![dispatch_copy(
                tx,
                ids,
                project.clone(),
                CopyCall::CopyStatus {
                    project: project.clone(),
                },
                PendingContinuation::Preview {
                    reply_id: id.clone(),
                    lease,
                },
            )?])
        }
        AdminRequest::Takeover {
            transfer_id,
            project,
            to_installation,
            reason,
            ..
        }
        | AdminRequest::Move {
            transfer_id,
            project,
            to_installation,
            reason,
        } => {
            let request_hash = format!(
                "{:x}",
                Sha256::digest(encode_record(&(subject, request))?.as_bytes())
            );
            if let Some(t) = load_transfer(tx, transfer_id)? {
                if t.request_hash != request_hash {
                    return Ok(refuse(id, AdminRefusal::ConflictingRequest));
                }
                return resume(tx, id, t, ids);
            }
            let Some(lease) = load_lease(tx, project)? else {
                return Ok(refuse(id, AdminRefusal::NotInitialized));
            };
            if let Some(t) = active_transfer(tx, project)? {
                return Ok(refuse(
                    id,
                    AdminRefusal::TransferInProgress {
                        transfer_id: t.transfer_id,
                    },
                ));
            }
            let (kind, state) = match request {
                AdminRequest::Takeover { confirm_token, .. } => {
                    let preview: Option<StoredPreview> = read_record(
                        tx,
                        "SELECT record FROM _registry_previews WHERE token=?",
                        confirm_token,
                    )?;
                    let Some(p) = preview else {
                        return Ok(vec![dispatch_copy(
                            tx,
                            ids,
                            project.clone(),
                            CopyCall::CopyStatus {
                                project: project.clone(),
                            },
                            PendingContinuation::RejectedPreview {
                                reply_id: id.clone(),
                                project: project.clone(),
                            },
                        )?]);
                    };
                    if p.project != *project || p.holder != lease.holder || p.epoch != lease.epoch {
                        return Ok(refuse(id, AdminRefusal::PreviewStale { acked: p.acked }));
                    }
                    let supported = load_machine(tx, to_installation)?.and_then(|m| {
                        if m.state == MachineState::Enrolled {
                            m.schema_support.map(|s| s.max_version)
                        } else {
                            None
                        }
                    });
                    let Some(max) = supported else {
                        return Ok(refuse(
                            id,
                            AdminRefusal::SchemaTooNew {
                                store: p.schema,
                                supported: None,
                            },
                        ));
                    };
                    let floor =
                        load_floor(tx, project)?.map_or(p.copy_fence, |e| e.max(p.copy_fence));
                    let Some(next) = floor
                        .value()
                        .checked_add(1)
                        .and_then(|e| Epoch::new(e).ok())
                    else {
                        return Ok(refuse(id, AdminRefusal::RegistryEpochUnproven));
                    };
                    (
                        TransferKind::Takeover,
                        TransferState::Fencing {
                            new_epoch: next,
                            expect: FenceExpectation {
                                epoch: p.copy_fence,
                                acked: p.acked,
                            },
                            target_schema_max: Some(max),
                            attempt: FenceAttempt::Initial,
                        },
                    )
                }
                AdminRequest::Move { .. } => {
                    let holder = load_machine(tx, &lease.holder)?;
                    if now.0 - lease.last_renewed_at >= TimeDelta::minutes(30)
                        || holder.as_ref().is_none_or(|m| m.connection.is_none())
                    {
                        return Ok(refuse(
                            id,
                            AdminRefusal::HolderUnreachable {
                                last_seen: holder.map_or(lease.last_renewed_at, |m| m.last_seen),
                            },
                        ));
                    }
                    (TransferKind::Move, TransferState::Draining)
                }
                _ => return Err(CentralError::ReplyContract),
            };
            let t = TransferRecord {
                transfer_id: transfer_id.clone(),
                project: project.clone(),
                kind,
                requester: subject.clone(),
                reason: reason.clone(),
                old_epoch: lease.epoch,
                to_installation: to_installation.clone(),
                state,
                request_hash,
                fleet_cleanup: FleetCleanup::Acknowledged,
            };
            save_transfer(tx, &t)?;
            let mut effects = resume(tx, id, t, ids)?;
            if matches!(request, AdminRequest::Move { .. }) {
                effects.push(notify(
                    &lease.holder,
                    NoticeKind::ReleaseRequested {
                        project: project.clone(),
                        transfer: transfer_id.clone(),
                    },
                ));
            }
            Ok(effects)
        }
    }
}
pub(crate) fn resume(
    tx: &mut dyn SqlTx,
    id: &ObjectCallId,
    t: TransferRecord,
    ids: &mut IdBatch,
) -> Result<Vec<RegistryEffect>, CentralError> {
    match &t.state {
        TransferState::ClaimGranted { .. }
        | TransferState::ClaimRefused(_)
        | TransferState::Reserving { .. } => Err(CentralError::ReplyContract),
        TransferState::Draining => Ok(returned(
            id,
            RegistryResult::Admin(AdminResult::Pending {
                transfer_id: t.transfer_id,
            }),
        )),
        TransferState::Fencing { .. } => {
            fence_dispatch(tx, TransferReply::Admin(id.clone()), &t, ids)
        }
        TransferState::Granted { new_epoch, start } => Ok(returned(
            id,
            RegistryResult::Admin(AdminResult::Granted {
                new_epoch: *new_epoch,
                start: *start,
            }),
        )),
        TransferState::Refused(reason) => Ok(refuse(id, reason.clone())),
    }
}
pub(crate) fn notify(
    installation: &collaboration_replication_protocol::InstallationId,
    kind: NoticeKind,
) -> RegistryEffect {
    RegistryEffect::Dispatch(RegistryDispatch::Notice {
        installation: installation.clone(),
        kind,
    })
}
pub(crate) fn fence_dispatch(
    tx: &mut dyn SqlTx,
    reply: TransferReply,
    t: &TransferRecord,
    ids: &mut IdBatch,
) -> Result<Vec<RegistryEffect>, CentralError> {
    let TransferState::Fencing {
        new_epoch,
        expect,
        target_schema_max,
        ..
    } = &t.state
    else {
        return Err(CentralError::ReplyContract);
    };
    let Some(max) = *target_schema_max else {
        return Ok(vec![dispatch_copy(
            tx,
            ids,
            t.project.clone(),
            CopyCall::CopyStatus {
                project: t.project.clone(),
            },
            PendingContinuation::ReleaseSchemaUnknown {
                reply,
                transfer_id: t.transfer_id.clone(),
            },
        )?]);
    };
    Ok(vec![dispatch_copy(
        tx,
        ids,
        t.project.clone(),
        CopyCall::Fence {
            transfer_id: t.transfer_id.clone(),
            new_epoch: *new_epoch,
            expect: expect.clone(),
            target_schema_max: max,
            holder: t.to_installation.clone(),
        },
        PendingContinuation::FenceTransfer {
            reply,
            transfer_id: t.transfer_id.clone(),
        },
    )?])
}
pub(crate) fn release(
    tx: &mut dyn SqlTx,
    id: &ObjectCallId,
    call: &RegistryCall,
    now: RegistryTime,
    ids: &mut IdBatch,
) -> Result<Vec<RegistryEffect>, CentralError> {
    let RegistryCall::Release {
        installation,
        project,
        epoch,
        transfer,
        outcome,
    } = call
    else {
        return Err(CentralError::ReplyContract);
    };
    let not_writer = || {
        returned(
            id,
            RegistryResult::Release(SessionAnswer::Refused(SessionRefusal::NotWriter {
                current: None,
            })),
        )
    };
    let Some(mut t) = load_transfer(tx, transfer)? else {
        return Ok(not_writer());
    };
    let history = tx.query(query_statement(
        "SELECT installation FROM _registry_history WHERE project=? AND epoch=?",
        vec![text(project.as_str()), unsigned(epoch.value())],
        ["installation"],
    ))?;
    let historical_holder = history
        .first()
        .map(|row| row.text("installation"))
        .transpose()?;
    if t.project != *project
        || t.old_epoch != *epoch
        || t.kind != TransferKind::Move
        || historical_holder != Some(installation.as_str())
    {
        return Ok(not_writer());
    }
    match &t.state {
        TransferState::Granted { start, .. } => {
            return Ok(returned(
                id,
                RegistryResult::Release(SessionAnswer::Released {
                    final_position: *start,
                }),
            ));
        }
        TransferState::Refused(AdminRefusal::MigrationPending(reason)) => {
            return Ok(returned(
                id,
                RegistryResult::Release(SessionAnswer::ReleaseRefused {
                    transfer: transfer.clone(),
                    reason: reason.clone(),
                }),
            ));
        }
        TransferState::Refused(AdminRefusal::SchemaTooNew { store, supported }) => {
            return Ok(returned(
                id,
                RegistryResult::Release(SessionAnswer::ReleaseRefused {
                    transfer: transfer.clone(),
                    reason: collaboration_replication_protocol::ReleaseRefusal::SchemaTooNew {
                        store: store.clone(),
                        supported: *supported,
                    },
                }),
            ));
        }
        TransferState::Fencing { .. } => {
            return fence_dispatch(tx, TransferReply::Release(id.clone()), &t, ids);
        }
        TransferState::Refused(_) => return Ok(not_writer()),
        TransferState::ClaimGranted { .. }
        | TransferState::ClaimRefused(_)
        | TransferState::Reserving { .. } => {
            return Ok(not_writer());
        }
        TransferState::Draining => {}
    }
    match outcome {
        ReleaseOutcome::Refused(reason) => {
            let reason: collaboration_replication_protocol::ReleaseRefusal = reason.clone().into();
            t.state = TransferState::Refused(AdminRefusal::MigrationPending(reason.clone()));
            save_transfer(tx, &t)?;
            Ok(returned(
                id,
                RegistryResult::Release(SessionAnswer::ReleaseRefused {
                    transfer: transfer.clone(),
                    reason,
                }),
            ))
        }
        ReleaseOutcome::Final(final_position) => {
            let supported = load_machine(tx, &t.to_installation)?
                .and_then(|m| m.schema_support.map(|s| s.max_version));
            let Some(max) = supported else {
                t.state = TransferState::Fencing {
                    new_epoch: *epoch,
                    expect: FenceExpectation {
                        epoch: *epoch,
                        acked: *final_position,
                    },
                    target_schema_max: None,
                    attempt: FenceAttempt::Initial,
                };
                // Get the authoritative copy schema before recording the refusal.
                save_transfer(tx, &t)?;
                return Ok(vec![dispatch_copy(
                    tx,
                    ids,
                    project.clone(),
                    CopyCall::CopyStatus {
                        project: project.clone(),
                    },
                    PendingContinuation::ReleaseSchemaUnknown {
                        reply: TransferReply::Release(id.clone()),
                        transfer_id: transfer.clone(),
                    },
                )?]);
            };
            let floor = load_floor(tx, project)?.map_or(*epoch, |e| e.max(*epoch));
            let next = floor
                .value()
                .checked_add(1)
                .and_then(|v| Epoch::new(v).ok())
                .ok_or_else(|| CentralError::InvalidInput {
                    message: "epoch exhausted".into(),
                })?;
            t.state = TransferState::Fencing {
                new_epoch: next,
                expect: FenceExpectation {
                    epoch: *epoch,
                    acked: *final_position,
                },
                target_schema_max: Some(max),
                attempt: FenceAttempt::Initial,
            };
            save_transfer(tx, &t)?;
            let _ = now;
            fence_dispatch(tx, TransferReply::Release(id.clone()), &t, ids)
        }
    }
}
