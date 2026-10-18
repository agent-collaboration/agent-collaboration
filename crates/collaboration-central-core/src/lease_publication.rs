//! Retry publication from current Registry rows; an old offer is never restamped.
use crate::lease_publish_storage::{self as stored, LeasePublication, PublicationState};
use crate::*;
use collaboration_replication_protocol::ProjectId;

pub(crate) fn changed(
    tx: &mut dyn SqlTx,
    project: &ProjectId,
    now: RegistryTime,
    ids: &mut IdBatch,
) -> Result<Vec<RegistryEffect>, CentralError> {
    stored::save(
        tx,
        &LeasePublication {
            project: project.clone(),
            state: PublicationState::Dirty { retry_at: now },
        },
    )?;
    drive(tx, now, ids)
}

pub(crate) fn drive(
    tx: &mut dyn SqlTx,
    now: RegistryTime,
    ids: &mut IdBatch,
) -> Result<Vec<RegistryEffect>, CentralError> {
    let owed = stored::owed(tx)?;
    if owed.is_empty() {
        return Ok(Vec::new());
    }
    let run = crate::lease_incarnation::run_id(ids)?;
    let mut effects = crate::lease_incarnation::drive(tx, now, ids)?;
    if let Some(incarnation) = crate::lease_incarnation::active(tx, &run)? {
        let mut remaining = ids.call_ids.len();
        for publication in owed {
            if publication.state.retry_at().0 > now.0 {
                continue;
            }
            if remaining == 0 {
                break;
            }
            remaining -= 1;
            let current = stored::current(tx, &publication.project)?;
            let version = LeaseVersion::new(current.epoch, incarnation, current.change.get())
                .map_err(|_| crate::fleet_storage::corrupt("current lease version"))?;
            let digest = LeaseContentDigest::of(&current.project, &current.lease, current.as_of)?;
            let settlement = stored::settlement(tx, &current.project)?;
            let existing = match &publication.state {
                PublicationState::Offered {
                    call_id,
                    run: prior_run,
                    version: prior,
                    digest: prior_digest,
                    settlement: prior_settlement,
                    ..
                } if *prior_run == run
                    && *prior == version
                    && *prior_digest == digest
                    && *prior_settlement == settlement =>
                {
                    Some(call_id.clone())
                }
                _ => None,
            };
            let call_id = if let Some(call_id) = existing {
                call_id
            } else {
                let Some(call_id) = ids.call_ids.pop() else {
                    break;
                };
                call_id
            };
            let update = match &settlement {
                Some(attempt) => DirectoryUpdate::claim_granted(
                    current.project.clone(),
                    attempt.clone(),
                    current.lease,
                    version,
                    current.as_of,
                )?,
                None => DirectoryUpdate::lease(
                    current.project.clone(),
                    current.lease,
                    version,
                    current.as_of,
                )?,
            };
            stored::save(
                tx,
                &LeasePublication {
                    project: current.project,
                    state: PublicationState::Offered {
                        call_id: call_id.clone(),
                        run: run.clone(),
                        version,
                        digest,
                        settlement,
                        retry_at: RegistryTime(now.0 + chrono::TimeDelta::seconds(5)),
                    },
                },
            )?;
            effects.push(RegistryEffect::Dispatch(RegistryDispatch::Fleet(
                Dispatch {
                    call_id,
                    to: (),
                    call: FleetCall::DirectoryUpdate(update),
                    carry: None,
                },
            )));
        }
    }
    if let Some(at) = next_alarm(tx)? {
        effects.push(RegistryEffect::SetAlarm { at });
    }
    let earliest = effects
        .iter()
        .filter_map(|effect| match effect {
            RegistryEffect::SetAlarm { at } => Some(*at),
            _ => None,
        })
        .min_by_key(|at| at.0);
    effects.retain(|effect| !matches!(effect, RegistryEffect::SetAlarm { .. }));
    if let Some(at) = earliest {
        effects.insert(0, RegistryEffect::SetAlarm { at });
    }
    Ok(effects)
}

pub(crate) fn result(
    tx: &mut dyn SqlTx,
    call_id: &ObjectCallId,
    result: &LeaseUpdateResult,
    now: RegistryTime,
    ids: &mut IdBatch,
) -> Result<Vec<RegistryEffect>, CentralError> {
    let Some(publication) = stored::owed(tx)?
        .into_iter()
        .find(|publication| publication.state.call_id() == Some(call_id))
    else {
        return Ok(Vec::new());
    };
    let PublicationState::Offered {
        run: offered_run,
        version,
        digest,
        settlement,
        ..
    } = &publication.state
    else {
        return Ok(Vec::new());
    };
    let run = crate::lease_incarnation::run_id(ids)?;
    if *offered_run != run
        || crate::lease_incarnation::active(tx, &run)? != Some(version.incarnation())
    {
        return Ok(Vec::new());
    }
    let current = stored::current(tx, &publication.project)?;
    if current.epoch != version.epoch()
        || current.change.get() != version.change()
        || LeaseContentDigest::of(&current.project, &current.lease, current.as_of)? != *digest
        || stored::settlement(tx, &publication.project)? != *settlement
    {
        return Ok(Vec::new());
    }
    match result {
        LeaseUpdateResult::Applied
        | LeaseUpdateResult::Duplicate
        | LeaseUpdateResult::Older { .. } => {
            if let Some(attempt) = settlement {
                stored::acknowledge_settlement(tx, &publication.project, attempt)?;
            }
            stored::remove(tx, &publication.project)?;
            Ok(Vec::new())
        }
        LeaseUpdateResult::Fenced { fence } if fence.value() <= version.incarnation().value() => {
            Ok(Vec::new())
        }
        LeaseUpdateResult::Fenced { fence } => {
            stored::save(
                tx,
                &LeasePublication {
                    project: publication.project,
                    state: PublicationState::Dirty { retry_at: now },
                },
            )?;
            crate::lease_incarnation::reacquire(tx, fence.value(), now, ids)
        }
        LeaseUpdateResult::Conflict {
            stored: stored_version,
        } => {
            stored::save(
                tx,
                &LeasePublication {
                    project: publication.project,
                    state: PublicationState::Dirty { retry_at: now },
                },
            )?;
            crate::lease_incarnation::reacquire(tx, stored_version.incarnation().value(), now, ids)
        }
    }
}

pub(crate) fn next_alarm(tx: &mut dyn SqlTx) -> Result<Option<RegistryTime>, CentralError> {
    let run_alarm = crate::lease_incarnation::next_alarm(tx)?;
    // While startup allocation is pending, publication waits for that result.
    if run_alarm.is_some() {
        return Ok(run_alarm);
    }
    Ok(stored::owed(tx)?
        .into_iter()
        .map(|publication| publication.state.retry_at())
        .min_by_key(|at| at.0))
}
