use crate::copy_storage::{encode_record, statement, text};
use crate::registry_storage::read_record;
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
enum RunState {
    Pending {
        at_least: u64,
    },
    Acquiring {
        at_least: u64,
        call_id: ObjectCallId,
        retry_at: RegistryTime,
    },
    Active {
        incarnation: RegistryIncarnation,
    },
}
impl RunState {
    fn lower_bound(&self) -> u64 {
        match self {
            Self::Pending { at_least } | Self::Acquiring { at_least, .. } => *at_least,
            Self::Active { incarnation } => incarnation.value(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PublicationRun {
    run: RegistryRunId,
    state: RunState,
}

fn load(tx: &mut dyn SqlTx) -> Result<Option<PublicationRun>, CentralError> {
    if !crate::registry_dispatch::has_table(tx, "_registry_lease_run")? {
        return Ok(None);
    }
    read_record(
        tx,
        "SELECT record FROM _registry_lease_run WHERE singleton=?",
        "1",
    )
}
fn save(tx: &mut dyn SqlTx, value: &PublicationRun) -> Result<(), CentralError> {
    tx.execute(statement("INSERT INTO _registry_lease_run(singleton,record) VALUES(1,?) ON CONFLICT(singleton) DO UPDATE SET record=excluded.record", vec![text(encode_record(value)?)]))?;
    Ok(())
}
pub(crate) fn run_id(ids: &IdBatch) -> Result<RegistryRunId, CentralError> {
    ids.registry_run
        .clone()
        .ok_or_else(|| CentralError::InvalidInput {
            message: "registry actor supplied no run identity".into(),
        })
}

pub(crate) fn active(
    tx: &mut dyn SqlTx,
    run: &RegistryRunId,
) -> Result<Option<RegistryIncarnation>, CentralError> {
    Ok(load(tx)?.and_then(|value| match value.state {
        RunState::Active { incarnation } if value.run == *run => Some(incarnation),
        _ => None,
    }))
}

pub(crate) fn drive(
    tx: &mut dyn SqlTx,
    now: RegistryTime,
    ids: &mut IdBatch,
) -> Result<Vec<RegistryEffect>, CentralError> {
    let run = run_id(ids)?;
    let prior = load(tx)?;
    let mut value = match prior {
        Some(value) if value.run == run => value,
        previous => PublicationRun {
            run,
            state: RunState::Pending {
                at_least: previous.map_or(0, |value| value.state.lower_bound()),
            },
        },
    };
    let at_least = match value.state {
        RunState::Active { .. } => return Ok(Vec::new()),
        RunState::Acquiring { retry_at, .. } if retry_at.0 > now.0 => return Ok(Vec::new()),
        RunState::Pending { at_least } | RunState::Acquiring { at_least, .. } => at_least,
    };
    let call_id = ids
        .call_ids
        .pop()
        .ok_or_else(|| CentralError::InvalidInput {
            message: "missing incarnation allocation ID".into(),
        })?;
    let retry_at = RegistryTime(now.0 + chrono::TimeDelta::seconds(5));
    value.state = RunState::Acquiring {
        at_least,
        call_id: call_id.clone(),
        retry_at,
    };
    save(tx, &value)?;
    Ok(vec![
        RegistryEffect::SetAlarm { at: retry_at },
        RegistryEffect::Dispatch(RegistryDispatch::Fleet(Dispatch {
            call_id,
            to: (),
            call: FleetCall::AdvanceLeaseIncarnation { at_least },
            carry: None,
        })),
    ])
}

pub(crate) fn acquired(
    tx: &mut dyn SqlTx,
    call_id: &ObjectCallId,
    incarnation: RegistryIncarnation,
    now: RegistryTime,
    ids: &mut IdBatch,
) -> Result<Vec<RegistryEffect>, CentralError> {
    let run = run_id(ids)?;
    let Some(mut value) = load(tx)? else {
        return Ok(Vec::new());
    };
    let RunState::Acquiring {
        call_id: expected,
        at_least,
        ..
    } = &value.state
    else {
        return Ok(Vec::new());
    };
    if value.run != run || expected != call_id {
        return Ok(Vec::new());
    }
    if incarnation.value() <= *at_least {
        return Err(CentralError::ReplyContract);
    }
    value.state = RunState::Active { incarnation };
    save(tx, &value)?;
    crate::lease_publication::drive(tx, now, ids)
}

pub(crate) fn reacquire(
    tx: &mut dyn SqlTx,
    floor: u64,
    now: RegistryTime,
    ids: &mut IdBatch,
) -> Result<Vec<RegistryEffect>, CentralError> {
    let run = run_id(ids)?;
    let Some(mut value) = load(tx)? else {
        return Ok(Vec::new());
    };
    if value.run != run {
        return Ok(Vec::new());
    }
    value.state = RunState::Pending {
        at_least: value.state.lower_bound().max(floor),
    };
    save(tx, &value)?;
    drive(tx, now, ids)
}

pub(crate) fn next_alarm(tx: &mut dyn SqlTx) -> Result<Option<RegistryTime>, CentralError> {
    Ok(load(tx)?.and_then(|value| match value.state {
        RunState::Acquiring { retry_at, .. } => Some(retry_at),
        _ => None,
    }))
}
