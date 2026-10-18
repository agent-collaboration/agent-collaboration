//! Terminal claim work remains owned by its transfer until Fleet acknowledges.
use crate::copy_storage::{decode_record, query_statement};
use crate::registry_contract::PendingContinuation;
use crate::registry_storage::{continuation_id, load_transfer, save_transfer};
use crate::*;

fn dispatch(
    call_id: ObjectCallId,
    project: &collaboration_replication_protocol::ProjectId,
    attempt: &collaboration_replication_protocol::RequestId,
) -> RegistryEffect {
    let call = FleetCall::ReleaseClaim {
        project: project.clone(),
        attempt: attempt.clone(),
    };
    RegistryEffect::Dispatch(RegistryDispatch::Fleet(Dispatch {
        call_id,
        to: (),
        call,
        carry: None,
    }))
}

pub(crate) fn grant(
    tx: &mut dyn SqlTx,
    transfer: &mut TransferRecord,
    now: RegistryTime,
    ids: &mut IdBatch,
) -> Result<Vec<RegistryEffect>, CentralError> {
    transfer.fleet_cleanup = FleetCleanup::Owed(ClaimCleanupDebt::Granted {
        project: transfer.project.clone(),
        attempt: collaboration_replication_protocol::RequestId::new(transfer.transfer_id.as_str())
            .map_err(|_| CentralError::ReplyContract)?,
    });
    save_transfer(tx, transfer)?;
    crate::lease_publication::changed(tx, &transfer.project, now, ids)
}

pub(crate) fn refused(
    tx: &mut dyn SqlTx,
    transfer: &mut TransferRecord,
    now: RegistryTime,
    ids: &mut IdBatch,
) -> Result<Vec<RegistryEffect>, CentralError> {
    let call_id = continuation_id(
        tx,
        ids,
        PendingContinuation::ClaimCleanup {
            transfer_id: transfer.transfer_id.clone(),
        },
    )?;
    let retry_at = RegistryTime(now.0 + chrono::TimeDelta::seconds(5));
    let project = transfer.project.clone();
    let attempt = collaboration_replication_protocol::RequestId::new(transfer.transfer_id.as_str())
        .map_err(|_| CentralError::ReplyContract)?;
    transfer.fleet_cleanup = FleetCleanup::Owed(ClaimCleanupDebt::Refused {
        project: project.clone(),
        attempt: attempt.clone(),
        call_id: call_id.clone(),
        retry_at,
    });
    save_transfer(tx, transfer)?;
    Ok(vec![
        RegistryEffect::SetAlarm { at: retry_at },
        dispatch(call_id, &project, &attempt),
    ])
}

pub(crate) fn acknowledged(
    tx: &mut dyn SqlTx,
    transfer_id: &collaboration_replication_protocol::TransferId,
) -> Result<(), CentralError> {
    let mut transfer = load_transfer(tx, transfer_id)?.ok_or(CentralError::ReplyContract)?;
    transfer.fleet_cleanup = FleetCleanup::Acknowledged;
    save_transfer(tx, &transfer)
}

fn owed(tx: &mut dyn SqlTx) -> Result<Vec<TransferRecord>, CentralError> {
    if !crate::registry_dispatch::has_table(tx, "_registry_transfers")? {
        return Ok(Vec::new());
    }
    tx.query(query_statement(
        "SELECT transfer_id,record FROM _registry_transfers",
        Vec::new(),
        ["transfer_id", "record"],
    ))?
    .iter()
    .filter_map(|row| {
        let parsed = (|| {
            let transfer: TransferRecord =
                decode_record(row.text("record")?, "claim cleanup transfer")?;
            if transfer.transfer_id.as_str() != row.text("transfer_id")? {
                return Err(crate::fleet_storage::corrupt(
                    "claim cleanup transfer identity",
                ));
            }
            Ok(transfer)
        })();
        match parsed {
            Ok(transfer)
                if matches!(
                    transfer.fleet_cleanup,
                    FleetCleanup::Owed(ClaimCleanupDebt::Refused { .. })
                ) =>
            {
                Some(Ok(transfer))
            }
            Ok(_) => None,
            Err(error) => Some(Err(error)),
        }
    })
    .collect()
}

pub(crate) fn next_alarm(tx: &mut dyn SqlTx) -> Result<Option<RegistryTime>, CentralError> {
    Ok(owed(tx)?
        .into_iter()
        .filter_map(|transfer| match transfer.fleet_cleanup {
            FleetCleanup::Owed(ClaimCleanupDebt::Refused { retry_at, .. }) => Some(retry_at),
            FleetCleanup::Acknowledged | FleetCleanup::Owed(ClaimCleanupDebt::Granted { .. }) => {
                None
            }
        })
        .min_by_key(|at| at.0))
}

pub(crate) fn alarm(
    tx: &mut dyn SqlTx,
    now: RegistryTime,
) -> Result<Vec<RegistryEffect>, CentralError> {
    let mut effects = Vec::new();
    for mut transfer in owed(tx)? {
        if let FleetCleanup::Owed(ClaimCleanupDebt::Refused {
            call_id,
            project,
            attempt,
            retry_at,
        }) = &mut transfer.fleet_cleanup
            && retry_at.0 <= now.0
        {
            *retry_at = RegistryTime(now.0 + chrono::TimeDelta::seconds(5));
            effects.push(dispatch(call_id.clone(), project, attempt));
            save_transfer(tx, &transfer)?;
        }
    }
    if let Some(at) = next_alarm(tx)? {
        effects.push(RegistryEffect::SetAlarm { at });
    }
    Ok(effects)
}
