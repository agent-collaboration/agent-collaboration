use crate::copy_storage::query_statement;
use crate::copy_storage::{decode_record, encode_record, statement, text};
use crate::registry_contract::PendingContinuation;
use crate::registry_storage::*;
use crate::*;
use crate::{
    CentralError, CredentialStatus, IdBatch, ObjectCallId, RegistrationResult, RegistryLease,
    RegistryTime, ReservationSource, SqlTx,
};
use chrono::TimeDelta;
use collaboration_replication_protocol::{
    CredentialRejection, Epoch, InstallationId, LeaseState, LeaseStatus, ProjectId, SessionAnswer,
    SessionRefusal,
};
pub(crate) fn returned(id: &ObjectCallId, result: RegistryResult) -> Vec<RegistryEffect> {
    vec![RegistryEffect::Return {
        call_id: id.clone(),
        result,
    }]
}
pub(crate) fn lease_status(lease: &RegistryLease, now: RegistryTime) -> LeaseStatus {
    let remaining = (lease.expires_at - now.0).num_milliseconds();
    LeaseStatus {
        project: lease.project.clone(),
        holder: lease.holder.clone(),
        epoch: lease.epoch,
        state: if remaining > 0 {
            LeaseState::Held
        } else {
            LeaseState::Expired
        },
        expires_at_registry: lease.expires_at,
        remaining_ms: remaining.max(0).unsigned_abs(),
    }
}
pub(crate) fn renew(
    tx: &mut dyn SqlTx,
    installation: &InstallationId,
    attempt: &collaboration_replication_protocol::RequestId,
    project: &ProjectId,
    epoch: Epoch,
    now: RegistryTime,
) -> Result<SessionAnswer, CentralError> {
    let lease = load_lease(tx, project)?;
    let stored: Option<RenewalAttempt> = read_record(
        tx,
        "SELECT record FROM _registry_renewal_attempts WHERE attempt=?",
        attempt.as_str(),
    )?;
    if let Some(stored) = stored {
        if &stored.installation != installation
            || &stored.project != project
            || stored.epoch != epoch
        {
            return Ok(SessionAnswer::Refused(SessionRefusal::ProjectRejected {
                reason: "conflictingRequest".into(),
            }));
        }
        return match stored.answer {
            SessionAnswer::Renewed { expires_at, .. } => {
                let lease = lease.ok_or_else(|| crate::StoreError::Corrupt {
                    field: "renewal current lease".into(),
                })?;
                Ok(SessionAnswer::Renewed {
                    expires_at,
                    replayed: true,
                    current: lease_status(&lease, now),
                    transfer_in_progress: active_transfer(tx, project)?.map(|t| t.transfer_id),
                })
            }
            answer => Ok(answer),
        };
    }
    let answer = if let Some(mut lease) = lease {
        if lease.holder == *installation && lease.epoch == epoch {
            lease.expires_at = now.0 + TimeDelta::hours(4);
            lease.last_renewed_at = now.0;
            save_lease(tx, &lease)?;
            SessionAnswer::Renewed {
                expires_at: lease.expires_at,
                replayed: false,
                current: lease_status(&lease, now),
                transfer_in_progress: active_transfer(tx, project)?.map(|t| t.transfer_id),
            }
        } else if let Some(cutoff) = cutoff(tx, project, epoch, installation)? {
            SessionAnswer::Refused(SessionRefusal::StaleEpoch {
                current_epoch: lease.epoch,
                epoch_ended_at: cutoff,
            })
        } else {
            SessionAnswer::Refused(SessionRefusal::NotWriter {
                current: Some(lease.epoch),
            })
        }
    } else {
        SessionAnswer::Refused(SessionRefusal::NotInitialized)
    };
    save_attempt(
        tx,
        attempt,
        &RenewalAttempt {
            installation: installation.clone(),
            project: project.clone(),
            epoch,
            answer: answer.clone(),
            decided_at: now.0,
        },
    )?;
    Ok(answer)
}
pub(crate) fn reserve(
    tx: &mut dyn SqlTx,
    id: &ObjectCallId,
    project: &ProjectId,
    source: &ReservationSource,
    now: RegistryTime,
    ids: &mut IdBatch,
) -> Result<Vec<RegistryEffect>, CentralError> {
    if let Some(lease) = load_lease(tx, project)? {
        return Ok(returned(
            id,
            RegistryResult::FirstLease(if &lease.holder == source.installation() {
                SessionAnswer::FirstLease {
                    epoch: lease.epoch,
                    expires_at: lease.expires_at,
                }
            } else {
                SessionAnswer::Refused(SessionRefusal::NotWriter {
                    current: Some(lease.epoch),
                })
            }),
        ));
    }
    if matches!(source, ReservationSource::Machine { .. }) {
        return Ok(returned(
            id,
            RegistryResult::FirstLease(SessionAnswer::Refused(SessionRefusal::NotInitialized)),
        ));
    }
    let reserved: Option<RegistryLease> = read_record(
        tx,
        "SELECT record FROM _registry_reservations WHERE project=?",
        project.as_str(),
    )?;
    let lease = if let Some(lease) = reserved {
        if &lease.holder != source.installation() {
            return Ok(returned(
                id,
                RegistryResult::FirstLease(SessionAnswer::Refused(SessionRefusal::NotWriter {
                    current: Some(lease.epoch),
                })),
            ));
        }
        lease
    } else {
        let lease = RegistryLease {
            change: std::num::NonZeroU64::MIN,
            project: project.clone(),
            holder: source.installation().clone(),
            epoch: Epoch::new(1).map_err(|e| CentralError::InvalidInput {
                message: e.to_string(),
            })?,
            expires_at: now.0 + TimeDelta::hours(4),
            last_renewed_at: now.0,
        };
        tx.execute(statement(
            "INSERT INTO _registry_reservations(project,record) VALUES(?,?)",
            vec![text(project.as_str()), text(encode_record(&lease)?)],
        ))?;
        lease
    };
    Ok(vec![dispatch_copy(
        tx,
        ids,
        project.clone(),
        CopyCall::Initialize {
            project: project.clone(),
            epoch: lease.epoch,
            holder: lease.holder.clone(),
        },
        PendingContinuation::Reserve {
            reply_id: id.clone(),
            lease,
        },
    )?])
}
pub(crate) fn register(
    tx: &mut dyn SqlTx,
    registration: &crate::SessionRegistration,
    now: RegistryTime,
) -> Result<RegistrationResult, CentralError> {
    let Some(mut machine) = load_machine(tx, &registration.installation)? else {
        return Ok(RegistrationResult::Rejected(CredentialRejection::Unknown));
    };
    if machine.state == MachineState::Revoked {
        return Ok(RegistrationResult::Rejected(CredentialRejection::Revoked));
    }
    if machine.expires_at <= now.0 {
        return Ok(RegistrationResult::Rejected(CredentialRejection::Expired));
    }
    machine.schema_support = Some(registration.schema_support);
    machine.connection = Some(registration.connection.clone());
    machine.last_seen = now.0;
    save_machine(tx, &machine)?;
    let mut projects = Vec::new();
    for row in tx.query(query_statement(
        "SELECT record FROM _registry_leases",
        Vec::new(),
        ["record"],
    ))? {
        let lease: RegistryLease = decode_record(row.text("record")?, "lease")?;
        if lease.holder == registration.installation {
            projects.push(lease_status(&lease, now));
        }
    }
    Ok(RegistrationResult::Registered { projects })
}
pub(crate) fn authenticate(
    tx: &mut dyn SqlTx,
    hash: &str,
    client_id: &crate::AccessClientId,
    now: RegistryTime,
) -> Result<CredentialStatus, CentralError> {
    let machine: Option<MachineRecord> = read_record(
        tx,
        "SELECT record FROM _registry_machines WHERE credential_hash=?",
        hash,
    )?;
    Ok(match machine {
        None => CredentialStatus::Rejected(CredentialRejection::Unknown),
        Some(m) if m.state == MachineState::Revoked => {
            CredentialStatus::Rejected(CredentialRejection::Revoked)
        }
        Some(m) if m.expires_at <= now.0 => {
            CredentialStatus::Rejected(CredentialRejection::Expired)
        }
        Some(m) if &m.access_client_id != client_id => {
            CredentialStatus::Rejected(CredentialRejection::NotMachine)
        }
        Some(m) => CredentialStatus::Machine {
            installation: m.installation,
            expires_at: m.expires_at,
        },
    })
}
