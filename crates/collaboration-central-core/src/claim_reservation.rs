use crate::copy_storage::{decode_record, encode_record, query_statement, statement, text};
use crate::*;
use collaboration_replication_protocol::{ProjectId, RequestId};
use message_board::{BoardState, Record};

fn read(tx: &mut dyn SqlTx, project: &ProjectId) -> Result<Vec<ClaimReservation>, CentralError> {
    tx.query(query_statement(
        "SELECT attempt,record FROM claim_reservations WHERE project=? ORDER BY attempt",
        vec![text(project.as_str())],
        ["attempt", "record"],
    ))?
    .iter()
    .map(|row| {
        let record: ClaimReservation = decode_record(row.text("record")?, "claim reservation")?;
        if record.project != *project || record.attempt.as_str() != row.text("attempt")? {
            return Err(crate::fleet_storage::corrupt("claim reservation identity"));
        }
        Ok(record)
    })
    .collect()
}

fn save(tx: &mut dyn SqlTx, record: &ClaimReservation) -> Result<(), CentralError> {
    tx.execute(statement(
        "INSERT INTO claim_reservations(project,attempt,record) VALUES(?,?,?) ON CONFLICT(project,attempt) DO UPDATE SET record=excluded.record",
        vec![text(record.project.as_str()), text(record.attempt.as_str()), text(encode_record(record)?)],
    ))?;
    Ok(())
}

pub(crate) fn active(tx: &mut dyn SqlTx, project: &ProjectId) -> Result<bool, CentralError> {
    Ok(read(tx, project)?
        .iter()
        .any(|record| record.state == ClaimReservationState::Active))
}

pub(crate) fn reserve(
    tx: &mut dyn SqlTx,
    project: &ProjectId,
    attempt: &RequestId,
) -> Result<ReserveClaimResult, CentralError> {
    if let Some(previous) = read(tx, project)?
        .into_iter()
        .find(|record| record.attempt == *attempt)
    {
        return Ok(match previous.state {
            ClaimReservationState::Active => ReserveClaimResult::Reserved,
            ClaimReservationState::Settled { outcome } => {
                ReserveClaimResult::AlreadySettled { outcome }
            }
        });
    }
    let project_reference = message_board::ProjectId::try_from(project.as_str().to_owned())
        .map_err(|_| crate::fleet_storage::corrupt("claim project"))?;
    let Some(Record::DirectoryEntry(entry)) =
        crate::fleet_storage::read_record(tx, &project_reference.into())?
    else {
        return Ok(ReserveClaimResult::Refused(
            ClaimReservationRefusal::NotFound,
        ));
    };
    let Some(Record::Board(board)) = crate::fleet_storage::read_record(tx, &entry.board.into())?
    else {
        return Err(crate::fleet_storage::corrupt("claim parent board"));
    };
    let refusal = match board.state {
        BoardState::Archiving => Some(ClaimReservationRefusal::BoardArchiving),
        BoardState::Archived => Some(ClaimReservationRefusal::BoardArchived),
        BoardState::Open | BoardState::Closed => None,
    };
    if let Some(refusal) = refusal {
        return Ok(ReserveClaimResult::Refused(refusal));
    }
    save(
        tx,
        &ClaimReservation {
            project: project.clone(),
            attempt: attempt.clone(),
            state: ClaimReservationState::Active,
        },
    )?;
    Ok(ReserveClaimResult::Reserved)
}

pub(crate) fn release(
    tx: &mut dyn SqlTx,
    project: &ProjectId,
    attempt: &RequestId,
    now: RegistryTime,
) -> Result<(), CentralError> {
    let previous = read(tx, project)?
        .into_iter()
        .find(|record| record.attempt == *attempt);
    if previous.is_none_or(|record| record.state == ClaimReservationState::Active) {
        save(
            tx,
            &ClaimReservation {
                project: project.clone(),
                attempt: attempt.clone(),
                state: ClaimReservationState::Settled {
                    outcome: ClaimReservationOutcome::Refused,
                },
            },
        )?;
    }
    crate::archive_receiver::complete_project(tx, project, now)
}

pub(crate) fn grant(
    tx: &mut dyn SqlTx,
    project: &ProjectId,
    attempt: &RequestId,
) -> Result<(), CentralError> {
    let previous = read(tx, project)?
        .into_iter()
        .find(|record| record.attempt == *attempt);
    match previous.map(|record| record.state) {
        Some(ClaimReservationState::Settled {
            outcome: ClaimReservationOutcome::Refused,
        }) => Err(crate::fleet_storage::corrupt(
            "grant for refused claim reservation",
        )),
        Some(ClaimReservationState::Settled {
            outcome: ClaimReservationOutcome::Granted,
        }) => Ok(()),
        None | Some(ClaimReservationState::Active) => save(
            tx,
            &ClaimReservation {
                project: project.clone(),
                attempt: attempt.clone(),
                state: ClaimReservationState::Settled {
                    outcome: ClaimReservationOutcome::Granted,
                },
            },
        ),
    }
}
