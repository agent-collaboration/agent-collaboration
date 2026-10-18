use crate::copy_storage::{encode_record, statement, text};
use crate::{CentralError, ObjectCallId, RegistryTime, SqlTx};
use fleet_admission::{
    FleetAdmissionError, FleetDecision, FleetIds, FleetRequest, FleetRequestResult, FleetWrite,
};
use message_board::{Record, RecordId};

pub(crate) fn decide(
    tx: &mut dyn SqlTx,
    request: &FleetRequest,
    id: &ObjectCallId,
    now: RegistryTime,
) -> Result<FleetRequestResult, CentralError> {
    let snapshot = crate::fleet_snapshot::read_snapshot(tx, fleet_admission::reads_for(request))?;
    let ids = FleetIds {
        generated_record: RecordId::try_from(id.as_str().to_owned())
            .map_err(|_| crate::fleet_storage::corrupt("fleet generated identity"))?,
    };
    let decision = match fleet_admission::decide(request, &snapshot, now.0, &ids) {
        Ok(decision) => decision,
        Err(FleetAdmissionError::ConflictingRequest) => {
            return Ok(FleetRequestResult::ConflictingRequest);
        }
        Err(error) => {
            return Err(CentralError::InvalidInput {
                message: error.to_string(),
            });
        }
    };
    apply(tx, &decision, now)?;
    Ok(FleetRequestResult::Decided(decision.decision))
}

fn apply(
    tx: &mut dyn SqlTx,
    decision: &FleetDecision,
    now: RegistryTime,
) -> Result<(), CentralError> {
    for write in &decision.writes {
        match write {
            FleetWrite::Put(record) => crate::fleet_storage::put_record(tx, record, None)?,
            FleetWrite::Ledger(ledger) => {
                crate::fleet_storage::put_record(
                    tx,
                    &Record::Request(ledger.as_ref().clone()),
                    None,
                )?;
                tx.execute(statement("INSERT INTO fleet_request_ledger(request_id,canonical_hash,payload) VALUES(?,?,?)", vec![text(ledger.request_id.as_str()),text(ledger.canonical_hash.as_str()),text(encode_record(ledger.as_ref())?)]))?;
            }
            FleetWrite::RepositoryAttach {
                workspace,
                repository,
            } => {
                tx.execute(statement(
                    "INSERT INTO fleet_repositories(workspace_id,reference) VALUES(?,?)",
                    vec![text(workspace.as_str()), text(encode_record(repository)?)],
                ))?;
            }
            FleetWrite::RepositoryDetach {
                workspace,
                repository,
            } => {
                tx.execute(statement(
                    "DELETE FROM fleet_repositories WHERE workspace_id=? AND reference=?",
                    vec![text(workspace.as_str()), text(encode_record(repository)?)],
                ))?;
            }
        }
    }
    crate::fleet_feed::append(tx, &decision.feed, now)
}
