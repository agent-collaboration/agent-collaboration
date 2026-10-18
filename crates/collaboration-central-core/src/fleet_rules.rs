use crate::*;
use collaboration_replication_protocol::{FleetPayload, ReadPage, ReadScope, SessionAnswer};

pub(crate) fn call(
    tx: &mut dyn SqlTx,
    id: &ObjectCallId,
    call: &FleetCall,
    now: RegistryTime,
) -> Result<Vec<FleetEffect>, CentralError> {
    crate::fleet_storage::bootstrap(tx)?;
    crate::fleet_feed::prune(tx, now)?;
    let result = match call {
        FleetCall::AdvanceLeaseIncarnation { at_least } => {
            FleetResult::LeaseIncarnationAdvanced(crate::lease_admission::advance(tx, *at_least)?)
        }
        FleetCall::ReserveClaim { project, attempt } => {
            FleetResult::ClaimReserved(crate::claim_reservation::reserve(tx, project, attempt)?)
        }
        FleetCall::ReleaseClaim { project, attempt } => {
            crate::claim_reservation::release(tx, project, attempt, now)?;
            FleetResult::ClaimReleased
        }
        FleetCall::Fleet {
            installation,
            request,
        } => {
            let parsed = serde_json::from_value::<fleet_admission::FleetRequest>(
                request.operation.as_value().clone(),
            );
            let parsed = match parsed {
                Ok(parsed)
                    if parsed.request_id.as_str() == request.request_id.as_str()
                        && parsed.actor.installation.as_str() == installation.as_str() =>
                {
                    parsed
                }
                _ => {
                    return Ok(vec![FleetEffect::Return {
                        call_id: id.clone(),
                        result: FleetResult::Fleet(SessionAnswer::Refused(
                            collaboration_replication_protocol::SessionRefusal::InvalidShape {
                                field: "fleetRequest".into(),
                            },
                        )),
                    }]);
                }
            };
            let decision = crate::fleet_request_admission::decide(tx, &parsed, id, now)?;
            if matches!(
                &decision,
                fleet_admission::FleetRequestResult::Decided(
                    message_board::RequestDecision::Accepted { .. }
                )
            ) {
                crate::archive_receiver::complete_request(tx, &parsed, now)?;
            }
            let accepted_project = match (&parsed.operation, &decision) {
                (
                    fleet_admission::FleetOperation::ProjectDirectoryCreate { project, .. },
                    fleet_admission::FleetRequestResult::Decided(
                        message_board::RequestDecision::Accepted { .. },
                    ),
                ) => Some(
                    collaboration_replication_protocol::ProjectId::new(project.as_str())
                        .map_err(|_| crate::fleet_storage::corrupt("accepted project identity"))?,
                ),
                _ => None,
            };
            let answer = SessionAnswer::FleetDecision {
                request_id: request.request_id.clone(),
                decision: FleetPayload::new(serde_json::to_value(decision).map_err(|error| {
                    CentralError::Encoding {
                        message: error.to_string(),
                    }
                })?),
            };
            match accepted_project {
                Some(project) => FleetResult::ProjectAccepted { project, answer },
                None => FleetResult::Fleet(answer),
            }
        }
        FleetCall::DirectoryUpdate(update) => match update {
            crate::DirectoryUpdate::Summary { .. } => {
                crate::directory_receiver::receive(tx, update)?;
                FleetResult::DirectoryUpdated
            }
            crate::DirectoryUpdate::Lease { .. } | crate::DirectoryUpdate::ClaimGranted { .. } => {
                FleetResult::LeaseUpdated(crate::lease_admission::receive(tx, update, now)?)
            }
        },
        FleetCall::ArchiveSeen {
            installation,
            board,
            project,
            position,
        } => {
            let invalid = || crate::fleet_storage::corrupt("archive report identity");
            FleetResult::ArchiveSeen(crate::archive_receiver::receive(
                tx,
                fleet_admission::ArchiveReport {
                    board: message_board::BoardId::try_from(board.as_str().to_owned())
                        .map_err(|_| invalid())?,
                    project: message_board::ProjectId::try_from(project.as_str().to_owned())
                        .map_err(|_| invalid())?,
                    position: *position,
                    installation: message_board::ServiceId::try_from(
                        installation.as_str().to_owned(),
                    )
                    .map_err(|_| invalid())?,
                    reported_at: now.0,
                },
            )?)
        }
        FleetCall::Import { bundle } => {
            let snapshot = crate::fleet_snapshot::read_snapshot(
                tx,
                fleet_admission::reads_for_import(bundle),
            )?;
            let decision = fleet_admission::decide_import(bundle, &snapshot).map_err(|error| {
                CentralError::InvalidInput {
                    message: error.to_string(),
                }
            })?;
            for write in &decision.writes {
                crate::fleet_storage::put_record(tx, &write.record, Some(&write.digest))?;
            }
            crate::fleet_feed::append(tx, &decision.feed, now)?;
            FleetResult::Imported(decision.result)
        }
        FleetCall::StoreTail {
            project,
            epoch,
            request_ids,
            digest,
            current_holder,
        } => FleetResult::TailReport(crate::fleet_tail_reports::store(
            tx,
            &collaboration_replication_protocol::TailReportRow {
                project: project.clone(),
                epoch: *epoch,
                request_ids: request_ids.clone(),
                digest: digest.clone(),
            },
            current_holder,
            now,
        )?),
        FleetCall::Read {
            scope: ReadScope::TailReports { project },
            after,
            ..
        } => FleetResult::Read(if after.is_some() {
            SessionAnswer::Refused(
                collaboration_replication_protocol::SessionRefusal::InvalidShape {
                    field: "after".into(),
                },
            )
        } else {
            SessionAnswer::ReadPage(ReadPage::TailReports {
                reports: crate::fleet_tail_reports::read(tx, project)?,
                as_of: now.0,
            })
        }),
        FleetCall::Read {
            scope: ReadScope::Directory { query },
            after,
            ..
        } => {
            let query =
                serde_json::from_value::<fleet_admission::DirectoryQuery>(query.as_value().clone());
            let answer = match (query, after) {
                (_, Some(_)) => SessionAnswer::Refused(
                    collaboration_replication_protocol::SessionRefusal::InvalidShape {
                        field: "after".into(),
                    },
                ),
                (Err(_), None) => SessionAnswer::Refused(
                    collaboration_replication_protocol::SessionRefusal::InvalidShape {
                        field: "directoryQuery".into(),
                    },
                ),
                (Ok(query), None) => {
                    let entries = crate::directory_read::read(tx, &query)?
                        .iter()
                        .map(|entry| {
                            serde_json::to_value(entry)
                                .map(FleetPayload::new)
                                .map_err(|error| CentralError::Encoding {
                                    message: error.to_string(),
                                })
                        })
                        .collect::<Result<_, _>>()?;
                    SessionAnswer::ReadPage(ReadPage::Directory {
                        entries,
                        as_of: now.0,
                    })
                }
            };
            FleetResult::Read(answer)
        }
        FleetCall::Read {
            installation,
            scope: ReadScope::FleetRecords,
            after,
        } => FleetResult::Read(crate::fleet_feed::read(
            tx,
            installation,
            after.as_ref(),
            now,
        )?),
        _ => {
            return Err(CentralError::InvalidInput {
                message: "Fleet request path not yet implemented in M3.5".into(),
            });
        }
    };
    Ok(vec![FleetEffect::Return {
        call_id: id.clone(),
        result,
    }])
}
