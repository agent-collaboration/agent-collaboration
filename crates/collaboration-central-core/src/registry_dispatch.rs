use crate::copy_storage::query_statement;
use crate::copy_storage::{statement, text};
use crate::registry_contract::PendingContinuation;
use crate::registry_rules::returned;
use crate::registry_storage::*;
use crate::*;
pub(crate) fn bootstrap(tx: &mut dyn SqlTx) -> Result<(), CentralError> {
    for sql in [
        include_str!("../migrations/0002_registry_storage.sql"),
        include_str!("../migrations/0007_registry_lease_publications.sql"),
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
pub(crate) fn call(
    tx: &mut dyn SqlTx,
    id: &ObjectCallId,
    call: &RegistryCall,
    now: RegistryTime,
    ids: &mut IdBatch,
) -> Result<Vec<RegistryEffect>, CentralError> {
    bootstrap(tx)?;
    match call {
        RegistryCall::VerifyTail {
            project,
            epoch,
            installation,
        } => Ok(returned(
            id,
            RegistryResult::TailVerification(crate::registry_tail_verification::verify(
                tx,
                project,
                *epoch,
                installation,
            )?),
        )),
        RegistryCall::Claim {
            attempt,
            project,
            claimant,
        } => crate::claim_rules::claim(tx, id, attempt, project, claimant, now, ids),
        RegistryCall::RecordUnheld(unheld) => {
            let mut effects = crate::claim_rules::record_unheld(tx, unheld, now, ids)?;
            effects.insert(
                0,
                RegistryEffect::Return {
                    call_id: id.clone(),
                    result: RegistryResult::UnheldRecorded,
                },
            );
            Ok(effects)
        }
        RegistryCall::ForwardRoute { project, sender } => Ok(returned(
            id,
            RegistryResult::ForwardRoute(crate::forward_rules::route(tx, project, sender)?),
        )),
        RegistryCall::Admin { authority, request } => {
            crate::transfer_rules::admin(tx, id, authority, request, now, ids)
        }
        RegistryCall::Renew {
            installation,
            attempt,
            project,
            epoch,
        } => {
            let answer =
                crate::registry_rules::renew(tx, installation, attempt, project, *epoch, now)?;
            let changed = matches!(
                &answer,
                collaboration_replication_protocol::SessionAnswer::Renewed {
                    replayed: false,
                    ..
                }
            );
            let mut effects = returned(id, RegistryResult::Renew(answer));
            if changed && let Some(lease) = load_lease(tx, project)? {
                effects.extend(crate::lease_publication::changed(
                    tx,
                    &lease.project,
                    now,
                    ids,
                )?);
            }
            Ok(effects)
        }
        RegistryCall::Release { .. } => crate::transfer_rules::release(tx, id, call, now, ids),
        RegistryCall::ReserveFirstEpoch { project, source } => {
            crate::registry_rules::reserve(tx, id, project, source, now, ids)
        }
        RegistryCall::RegisterSession(registration) => Ok(returned(
            id,
            RegistryResult::Registered(crate::registry_rules::register(tx, registration, now)?),
        )),
        RegistryCall::SessionDisconnected {
            installation,
            connection,
        } => {
            if let Some(mut m) = load_machine(tx, installation)?
                && m.connection.as_ref() == Some(connection)
            {
                m.connection = None;
                save_machine(tx, &m)?;
            }
            Ok(returned(id, RegistryResult::SessionDisconnected))
        }
        RegistryCall::AuthenticateMachine {
            credential_hash,
            access_client_id,
        } => Ok(returned(
            id,
            RegistryResult::Authenticated(crate::registry_rules::authenticate(
                tx,
                credential_hash,
                access_client_id,
                now,
            )?),
        )),
    }
}
pub(crate) fn continuation(
    tx: &mut dyn SqlTx,
    pending: PendingContinuation,
    result: RegistryReply,
    now: RegistryTime,
    ids: &mut IdBatch,
) -> Result<Vec<RegistryEffect>, CentralError> {
    if matches!(result, RegistryReply::Copy(CopyResult::Retired)) {
        return retired(pending);
    }
    if let RegistryReply::Copy(CopyResult::Unavailable(reason)) = result {
        return unavailable(pending, reason);
    }
    match (pending, result) {
        (
            PendingContinuation::ClaimReservation {
                reply_id,
                transfer_id,
            },
            RegistryReply::Fleet(FleetResult::ClaimReserved(result)),
        ) => crate::claim_rules::reservation_result(tx, &reply_id, &transfer_id, result, now, ids),
        (
            PendingContinuation::ClaimCleanup { transfer_id },
            RegistryReply::Fleet(FleetResult::ClaimReleased),
        ) => {
            crate::claim_cleanup::acknowledged(tx, &transfer_id)?;
            Ok(Vec::new())
        }
        (
            PendingContinuation::ImportProject { reply_id, .. },
            RegistryReply::Copy(CopyResult::ImportSeed(imported)),
        ) => {
            let mut effects = if let Some(unheld) = &imported.unheld {
                crate::claim_rules::record_unheld(tx, unheld, now, ids)?
            } else {
                Vec::new()
            };
            effects.insert(
                0,
                RegistryEffect::Return {
                    call_id: reply_id,
                    result: RegistryResult::Admin(AdminResult::ProjectImport(imported.answer)),
                },
            );
            Ok(effects)
        }
        (
            PendingContinuation::ClaimSchema {
                reply_id,
                attempt,
                unheld,
                claimant,
            },
            RegistryReply::Copy(CopyResult::CopyStatus(status)),
        ) => crate::claim_rules::schema_result(tx, &reply_id, &attempt, &unheld, &claimant, status),
        (
            PendingContinuation::ClaimRestore {
                reply_id,
                transfer_id,
            },
            RegistryReply::Copy(CopyResult::CopyStatus(status)),
        ) => crate::claim_rules::restore_result(tx, &reply_id, &transfer_id, status, now, ids),
        (
            PendingContinuation::RejectedPreview { reply_id, .. },
            RegistryReply::Copy(CopyResult::CopyStatus(status)),
        ) => Ok(returned(
            &reply_id,
            RegistryResult::Admin(AdminResult::Refused(match status {
                Some(s) => AdminRefusal::PreviewStale { acked: s.acked },
                None => AdminRefusal::RegistryEpochUnproven,
            })),
        )),
        (
            PendingContinuation::ReleaseSchemaUnknown { reply, transfer_id },
            RegistryReply::Copy(CopyResult::CopyStatus(status)),
        ) => {
            let s = status.ok_or_else(|| StoreError::Corrupt {
                field: "release schema".into(),
            })?;
            let mut t = load_transfer(tx, &transfer_id)?.ok_or_else(|| StoreError::Corrupt {
                field: "release transfer".into(),
            })?;
            t.state = TransferState::Refused(AdminRefusal::SchemaTooNew {
                store: s.schema,
                supported: None,
            });
            save_transfer(tx, &t)?;
            crate::transfer_completion::reply_transfer(&reply, &t)
        }
        (
            PendingContinuation::Reserve { reply_id, lease },
            RegistryReply::Copy(CopyResult::Initialized(status)),
        ) => {
            if status.holder.as_ref() != Some(&lease.holder) {
                return Ok(returned(
                    &reply_id,
                    RegistryResult::FirstLease(
                        collaboration_replication_protocol::SessionAnswer::Refused(
                            collaboration_replication_protocol::SessionRefusal::NotHeld,
                        ),
                    ),
                ));
            }
            if status.project != lease.project || status.fence != lease.epoch {
                return Err(StoreError::Corrupt {
                    field: "initialization reply".into(),
                }
                .into());
            }
            if load_lease(tx, &lease.project)?.is_none() {
                save_lease(tx, &lease)?;
            }
            let mut effects = returned(
                &reply_id,
                RegistryResult::FirstLease(
                    collaboration_replication_protocol::SessionAnswer::FirstLease {
                        epoch: lease.epoch,
                        expires_at: lease.expires_at,
                    },
                ),
            );
            effects.extend(crate::lease_publication::changed(
                tx,
                &lease.project,
                now,
                ids,
            )?);
            Ok(effects)
        }
        (
            PendingContinuation::Preview { reply_id, lease },
            RegistryReply::Copy(CopyResult::CopyStatus(status)),
        ) => crate::transfer_completion::preview_result(tx, &reply_id, lease, status, now),
        (
            PendingContinuation::FenceTransfer { reply, transfer_id },
            RegistryReply::Copy(CopyResult::Fence(result)),
        ) => crate::transfer_completion::fence_result(tx, reply, &transfer_id, result, now, ids),
        _ => Err(CentralError::ReplyContract),
    }
}
fn retired(pending: PendingContinuation) -> Result<Vec<RegistryEffect>, CentralError> {
    use collaboration_replication_protocol::{SessionAnswer, SessionRefusal};
    let (reply_id, result) = match pending {
        PendingContinuation::Reserve { reply_id, .. } => (
            reply_id,
            RegistryResult::FirstLease(SessionAnswer::Refused(SessionRefusal::Retired)),
        ),
        PendingContinuation::ImportProject { reply_id, .. }
        | PendingContinuation::RejectedPreview { reply_id, .. }
        | PendingContinuation::Preview { reply_id, .. } => {
            (reply_id, RegistryResult::Admin(AdminResult::Retired {}))
        }
        PendingContinuation::ClaimSchema { reply_id, .. }
        | PendingContinuation::ClaimRestore { reply_id, .. } => (
            reply_id,
            RegistryResult::Claim(SessionAnswer::Refused(SessionRefusal::Retired)),
        ),
        PendingContinuation::ReleaseSchemaUnknown { reply, .. }
        | PendingContinuation::FenceTransfer { reply, .. } => match reply {
            crate::registry_contract::TransferReply::Claim(reply_id) => (
                reply_id,
                RegistryResult::Claim(SessionAnswer::Refused(SessionRefusal::Retired)),
            ),
            crate::registry_contract::TransferReply::Release(reply_id) => (
                reply_id,
                RegistryResult::Release(SessionAnswer::Refused(SessionRefusal::Retired)),
            ),
            crate::registry_contract::TransferReply::Admin(reply_id) => {
                (reply_id, RegistryResult::Admin(AdminResult::Retired {}))
            }
        },
        _ => return Err(CentralError::ReplyContract),
    };
    Ok(returned(&reply_id, result))
}
fn unavailable(
    pending: PendingContinuation,
    reason: collaboration_replication_protocol::CopyUnavailableReason,
) -> Result<Vec<RegistryEffect>, CentralError> {
    use collaboration_replication_protocol::{BlockedSubject, SessionAnswer, TransferId};
    // Only the transport continuation is consumed. The persisted operation remains retryable.
    let (reply_id, result) = match pending {
        PendingContinuation::Reserve { reply_id, lease } => (
            reply_id,
            RegistryResult::FirstLease(SessionAnswer::Unavailable {
                subject: BlockedSubject::Project {
                    project: lease.project,
                },
                reason,
            }),
        ),
        PendingContinuation::ImportProject { reply_id, project }
        | PendingContinuation::RejectedPreview { reply_id, project } => (
            reply_id,
            RegistryResult::Admin(AdminResult::Unavailable {
                subject: BlockedSubject::Project { project },
                reason,
            }),
        ),
        PendingContinuation::Preview { reply_id, lease } => (
            reply_id,
            RegistryResult::Admin(AdminResult::Unavailable {
                subject: BlockedSubject::Project {
                    project: lease.project,
                },
                reason,
            }),
        ),
        PendingContinuation::ClaimSchema {
            reply_id, attempt, ..
        } => (
            reply_id,
            RegistryResult::Claim(SessionAnswer::Unavailable {
                subject: BlockedSubject::Transfer {
                    transfer: TransferId::new(attempt.as_str())
                        .map_err(|_| CentralError::ReplyContract)?,
                },
                reason,
            }),
        ),
        PendingContinuation::ClaimRestore {
            reply_id,
            transfer_id,
        } => (
            reply_id,
            RegistryResult::Claim(SessionAnswer::Unavailable {
                subject: BlockedSubject::Transfer {
                    transfer: transfer_id,
                },
                reason,
            }),
        ),
        PendingContinuation::ReleaseSchemaUnknown { reply, transfer_id }
        | PendingContinuation::FenceTransfer { reply, transfer_id } => {
            let subject = BlockedSubject::Transfer {
                transfer: transfer_id,
            };
            match reply {
                crate::registry_contract::TransferReply::Claim(reply_id) => (
                    reply_id,
                    RegistryResult::Claim(SessionAnswer::Unavailable { subject, reason }),
                ),
                crate::registry_contract::TransferReply::Release(reply_id) => (
                    reply_id,
                    RegistryResult::Release(SessionAnswer::Unavailable { subject, reason }),
                ),
                crate::registry_contract::TransferReply::Admin(reply_id) => (
                    reply_id,
                    RegistryResult::Admin(AdminResult::Unavailable { subject, reason }),
                ),
            }
        }
        _ => return Err(CentralError::ReplyContract),
    };
    Ok(returned(&reply_id, result))
}
pub(crate) fn has_table(tx: &mut dyn SqlTx, name: &str) -> Result<bool, CentralError> {
    Ok(!tx
        .query(query_statement(
            "SELECT name FROM sqlite_schema WHERE type='table' AND name=?",
            vec![text(name)],
            ["name"],
        ))?
        .is_empty())
}
