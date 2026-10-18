use crate::copy_storage::{encode_record, statement, text};
use crate::registry_contract::{PendingContinuation, TransferReply};
use crate::registry_storage::*;
use crate::*;
use collaboration_replication_protocol::*;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
struct RejectedClaim {
    project: ProjectId,
    claimant: InstallationId,
    answer: SessionAnswer,
}
fn bootstrap(tx: &mut dyn SqlTx) -> Result<(), CentralError> {
    tx.execute(statement("CREATE TABLE IF NOT EXISTS _registry_unheld (project TEXT PRIMARY KEY,record TEXT NOT NULL)",Vec::new()))?;
    tx.execute(statement("CREATE TABLE IF NOT EXISTS _registry_claim_rejections (attempt TEXT PRIMARY KEY,project TEXT NOT NULL,record TEXT NOT NULL)",Vec::new()))?;
    Ok(())
}
fn answer(id: &ObjectCallId, body: SessionAnswer) -> Vec<RegistryEffect> {
    crate::registry_rules::returned(id, RegistryResult::Claim(body))
}
fn reject(
    tx: &mut dyn SqlTx,
    id: &ObjectCallId,
    attempt: &RequestId,
    project: &ProjectId,
    claimant: &InstallationId,
    reason: SessionRefusal,
) -> Result<Vec<RegistryEffect>, CentralError> {
    let body = SessionAnswer::Refused(reason);
    tx.execute(statement("INSERT INTO _registry_claim_rejections(attempt,project,record) VALUES(?,?,?) ON CONFLICT(attempt) DO NOTHING",vec![text(attempt.as_str()),text(project.as_str()),text(encode_record(&RejectedClaim{project:project.clone(),claimant:claimant.clone(),answer:body.clone()})?)]))?;
    Ok(answer(id, body))
}
pub(crate) fn record_unheld(
    tx: &mut dyn SqlTx,
    unheld: &UnheldProject,
    now: RegistryTime,
    ids: &mut IdBatch,
) -> Result<Vec<RegistryEffect>, CentralError> {
    bootstrap(tx)?;
    if load_lease(tx, &unheld.project)?.is_some()
        || !tx
            .query(crate::copy_storage::query_statement(
                "SELECT transfer_id FROM _registry_transfers WHERE project=?",
                vec![text(unheld.project.as_str())],
                ["transfer_id"],
            ))?
            .is_empty()
    {
        return Ok(Vec::new());
    }
    let existing = crate::lease_publish_storage::unheld(tx, &unheld.project)?;
    if existing.is_some() {
        return Ok(Vec::new());
    }
    crate::lease_publish_storage::store_unheld(tx, unheld, now)?;
    let floor = load_floor(tx, &unheld.project)?
        .map_or(unheld.import_epoch, |floor| floor.max(unheld.import_epoch));
    save_floor(tx, &unheld.project, floor)?;
    crate::lease_publication::changed(tx, &unheld.project, now, ids)
}
pub(crate) fn claim(
    tx: &mut dyn SqlTx,
    id: &ObjectCallId,
    attempt: &RequestId,
    project: &ProjectId,
    claimant: &InstallationId,
    now: RegistryTime,
    ids: &mut IdBatch,
) -> Result<Vec<RegistryEffect>, CentralError> {
    bootstrap(tx)?;
    let rejected: Option<RejectedClaim> = read_record(
        tx,
        "SELECT record FROM _registry_claim_rejections WHERE attempt=?",
        attempt.as_str(),
    )?;
    if let Some(rejected) = rejected {
        if rejected.project != *project || rejected.claimant != *claimant {
            return Ok(answer(
                id,
                SessionAnswer::Refused(SessionRefusal::ProjectRejected {
                    reason: "conflictingRequest".into(),
                }),
            ));
        }
        return Ok(answer(id, rejected.answer));
    }
    let transfer_id =
        TransferId::new(attempt.as_str()).map_err(|e| CentralError::InvalidInput {
            message: e.to_string(),
        })?;
    let request_hash = encode_record(&(project, claimant))?;
    if let Some(transfer) = load_transfer(tx, &transfer_id)? {
        if transfer.kind != TransferKind::Claim || transfer.request_hash != request_hash {
            return Ok(answer(
                id,
                SessionAnswer::Refused(SessionRefusal::ProjectRejected {
                    reason: "conflictingRequest".into(),
                }),
            ));
        }
        return resume(tx, id, &transfer, ids);
    }
    if let Some(transfer) = active_transfer(tx, project)? {
        return reject(
            tx,
            id,
            attempt,
            project,
            claimant,
            SessionRefusal::AlreadyHeld {
                holder: transfer.to_installation,
            },
        );
    }
    if let Some(lease) = load_lease(tx, project)? {
        return reject(
            tx,
            id,
            attempt,
            project,
            claimant,
            SessionRefusal::AlreadyHeld {
                holder: lease.holder,
            },
        );
    }
    let unheld = crate::lease_publish_storage::unheld(tx, project)?.map(|stored| stored.unheld);
    let Some(unheld) = unheld else {
        return reject(tx, id, attempt, project, claimant, SessionRefusal::NotFound);
    };
    let support = load_machine(tx, claimant)?
        .filter(|machine| machine.state == MachineState::Enrolled && machine.expires_at > now.0)
        .and_then(|machine| machine.schema_support);
    let Some(support) = support.filter(|support| support.max_version >= unheld.schema_version)
    else {
        return Ok(vec![dispatch_copy(
            tx,
            ids,
            project.clone(),
            CopyCall::CopyStatus {
                project: project.clone(),
            },
            PendingContinuation::ClaimSchema {
                reply_id: id.clone(),
                attempt: attempt.clone(),
                unheld,
                claimant: claimant.clone(),
            },
        )?]);
    };
    let floor = load_floor(tx, project)?
        .map_or(unheld.import_epoch, |floor| floor.max(unheld.import_epoch));
    let epoch = floor
        .value()
        .checked_add(1)
        .and_then(|value| Epoch::new(value).ok())
        .ok_or_else(|| CentralError::InvalidInput {
            message: "claim epoch exhausted".into(),
        })?;
    let transfer = TransferRecord {
        transfer_id,
        project: project.clone(),
        kind: TransferKind::Claim,
        requester: claimant.as_str().into(),
        reason: "claim imported project".into(),
        old_epoch: unheld.import_epoch,
        to_installation: claimant.clone(),
        state: TransferState::Reserving {
            new_epoch: epoch,
            expect: FenceExpectation {
                epoch: unheld.import_epoch,
                acked: unheld.imported_head,
            },
            target_schema_max: support.max_version,
        },
        request_hash,
        fleet_cleanup: FleetCleanup::Acknowledged,
    };
    save_transfer(tx, &transfer)?;
    resume(tx, id, &transfer, ids)
}
fn resume(
    tx: &mut dyn SqlTx,
    id: &ObjectCallId,
    transfer: &TransferRecord,
    ids: &mut IdBatch,
) -> Result<Vec<RegistryEffect>, CentralError> {
    match &transfer.state {
        TransferState::Reserving { .. } => Ok(vec![RegistryEffect::Dispatch(
            RegistryDispatch::Fleet(Dispatch {
                call_id: continuation_id(
                    tx,
                    ids,
                    PendingContinuation::ClaimReservation {
                        reply_id: id.clone(),
                        transfer_id: transfer.transfer_id.clone(),
                    },
                )?,
                to: (),
                call: FleetCall::ReserveClaim {
                    project: transfer.project.clone(),
                    attempt: RequestId::new(transfer.transfer_id.as_str())
                        .map_err(|_| CentralError::ReplyContract)?,
                },
                carry: None,
            }),
        )]),
        TransferState::Fencing { .. } => crate::transfer_rules::fence_dispatch(
            tx,
            TransferReply::Claim(id.clone()),
            transfer,
            ids,
        ),
        _ => reply(id, transfer),
    }
}
pub(crate) fn reply(
    id: &ObjectCallId,
    transfer: &TransferRecord,
) -> Result<Vec<RegistryEffect>, CentralError> {
    Ok(answer(
        id,
        match &transfer.state {
            TransferState::ClaimGranted {
                epoch,
                expires_at,
                start,
            } => SessionAnswer::Claimed {
                epoch: *epoch,
                expires_at: *expires_at,
                start: *start,
            },
            TransferState::ClaimRefused(reason) => SessionAnswer::Refused(reason.clone()),
            _ => return Err(CentralError::ReplyContract),
        },
    ))
}
pub(crate) fn schema_result(
    tx: &mut dyn SqlTx,
    id: &ObjectCallId,
    attempt: &RequestId,
    unheld: &UnheldProject,
    claimant: &InstallationId,
    status: Option<CopyStatus>,
) -> Result<Vec<RegistryEffect>, CentralError> {
    let status = status.ok_or(CentralError::ReplyContract)?;
    let supported = load_machine(tx, claimant)?
        .and_then(|machine| machine.schema_support.map(|support| support.max_version));
    reject(
        tx,
        id,
        attempt,
        &unheld.project,
        claimant,
        SessionRefusal::SchemaTooNew {
            store: status.schema,
            supported,
        },
    )
}
pub(crate) fn fence_result(
    tx: &mut dyn SqlTx,
    id: &ObjectCallId,
    transfer_id: &TransferId,
    result: FenceResult,
    now: RegistryTime,
    ids: &mut IdBatch,
) -> Result<Vec<RegistryEffect>, CentralError> {
    let mut transfer = load_transfer(tx, transfer_id)?.ok_or(CentralError::ReplyContract)?;
    if matches!(
        transfer.state,
        TransferState::ClaimGranted { .. } | TransferState::ClaimRefused(_)
    ) {
        return reply(id, &transfer);
    }
    match result {
        FenceResult::Fenced {
            new_epoch, start, ..
        } => {
            let lease = RegistryLease {
                change: std::num::NonZeroU64::MIN,
                project: transfer.project.clone(),
                holder: transfer.to_installation.clone(),
                epoch: new_epoch,
                expires_at: now.0 + chrono::TimeDelta::hours(4),
                last_renewed_at: now.0,
            };
            save_lease(tx, &lease)?;
            tx.execute(statement(
                "DELETE FROM _registry_unheld WHERE project=?",
                vec![text(transfer.project.as_str())],
            ))?;
            transfer.state = TransferState::ClaimGranted {
                epoch: new_epoch,
                expires_at: lease.expires_at,
                start,
            };
            save_transfer(tx, &transfer)?;
            let mut effects = reply(id, &transfer)?;
            effects.push(crate::transfer_rules::notify(
                &transfer.to_installation,
                NoticeKind::LeaseGranted {
                    project: transfer.project.clone(),
                    epoch: new_epoch,
                },
            ));
            effects.extend(crate::claim_cleanup::grant(tx, &mut transfer, now, ids)?);
            Ok(effects)
        }
        FenceResult::PreviewStale { .. } | FenceResult::FenceNotAdvanced { .. } => {
            Ok(vec![dispatch_copy(
                tx,
                ids,
                transfer.project.clone(),
                CopyCall::CopyStatus {
                    project: transfer.project.clone(),
                },
                PendingContinuation::ClaimRestore {
                    reply_id: id.clone(),
                    transfer_id: transfer_id.clone(),
                },
            )?])
        }
        FenceResult::SchemaTooNew { store, supported } => {
            transfer.state = TransferState::ClaimRefused(SessionRefusal::SchemaTooNew {
                store,
                supported: Some(supported),
            });
            save_transfer(tx, &transfer)?;
            refused(tx, id, &mut transfer, now, ids)
        }
        FenceResult::NotInitialized => {
            transfer.state = TransferState::ClaimRefused(SessionRefusal::NotFound);
            save_transfer(tx, &transfer)?;
            refused(tx, id, &mut transfer, now, ids)
        }
    }
}
pub(crate) fn restore_result(
    tx: &mut dyn SqlTx,
    id: &ObjectCallId,
    transfer_id: &TransferId,
    status: Option<CopyStatus>,
    now: RegistryTime,
    ids: &mut IdBatch,
) -> Result<Vec<RegistryEffect>, CentralError> {
    let holder = status
        .and_then(|status| status.holder)
        .ok_or_else(|| StoreError::Corrupt {
            field: "claim restore holder".into(),
        })?;
    let mut transfer = load_transfer(tx, transfer_id)?.ok_or(CentralError::ReplyContract)?;
    transfer.state = TransferState::ClaimRefused(SessionRefusal::AlreadyHeld { holder });
    save_transfer(tx, &transfer)?;
    refused(tx, id, &mut transfer, now, ids)
}

fn refused(
    tx: &mut dyn SqlTx,
    id: &ObjectCallId,
    transfer: &mut TransferRecord,
    now: RegistryTime,
    ids: &mut IdBatch,
) -> Result<Vec<RegistryEffect>, CentralError> {
    let mut effects = reply(id, transfer)?;
    effects.extend(crate::claim_cleanup::refused(tx, transfer, now, ids)?);
    Ok(effects)
}

pub(crate) fn reservation_result(
    tx: &mut dyn SqlTx,
    id: &ObjectCallId,
    transfer_id: &TransferId,
    result: ReserveClaimResult,
    now: RegistryTime,
    ids: &mut IdBatch,
) -> Result<Vec<RegistryEffect>, CentralError> {
    let mut transfer = load_transfer(tx, transfer_id)?.ok_or(CentralError::ReplyContract)?;
    let TransferState::Reserving {
        new_epoch,
        expect,
        target_schema_max,
    } = transfer.state.clone()
    else {
        return resume(tx, id, &transfer, ids);
    };
    match result {
        ReserveClaimResult::Reserved
        | ReserveClaimResult::AlreadySettled {
            outcome: ClaimReservationOutcome::Granted,
        } => {
            transfer.state = TransferState::Fencing {
                new_epoch,
                expect,
                target_schema_max: Some(target_schema_max),
                attempt: FenceAttempt::Initial,
            };
            save_transfer(tx, &transfer)?;
            resume(tx, id, &transfer, ids)
        }
        ReserveClaimResult::Refused(reason) => {
            transfer.state = TransferState::ClaimRefused(match reason {
                ClaimReservationRefusal::BoardArchiving
                | ClaimReservationRefusal::BoardArchived => SessionRefusal::Archived,
                ClaimReservationRefusal::NotFound => SessionRefusal::NotFound,
            });
            save_transfer(tx, &transfer)?;
            refused(tx, id, &mut transfer, now, ids)
        }
        ReserveClaimResult::AlreadySettled {
            outcome: ClaimReservationOutcome::Refused,
        } => Err(CentralError::ReplyContract),
    }
}
