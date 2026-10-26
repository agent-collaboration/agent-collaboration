use std::collections::BTreeMap;
use std::path::Path;
use std::process::Child;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::bounded_command::{exit_code, wait_for_child};
use crate::driver_error::{DriverError, DriverResult};
use crate::driver_records::save_record;
use crate::process_identity::{ProcessIdentity, lookup_identity, observe_owned, process_table};

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum CleanupInventory {
    Observed {
        recorded: Vec<ProcessIdentity>,
        matching: Vec<ProcessIdentity>,
    },
    Unavailable {
        recorded: Vec<ProcessIdentity>,
        error: String,
    },
}
pub fn capture_cleanup_inventory(
    known: &BTreeMap<i32, ProcessIdentity>,
    read_processes: impl FnOnce() -> DriverResult<BTreeMap<i32, ProcessIdentity>>,
) -> CleanupInventory {
    let recorded = known.values().cloned().collect();
    let observed = read_processes().and_then(|observed| {
        let own_pid = i32::try_from(std::process::id())
            .map_err(|_| DriverError::Runtime("driver PID overflow"))?;
        if !observed.contains_key(&own_pid) {
            return Err(DriverError::Runtime(
                "process inventory omitted the active verifier",
            ));
        }
        Ok(observed)
    });
    match observed {
        Ok(observed) => CleanupInventory::Observed {
            recorded,
            matching: observed
                .into_iter()
                .filter(|(pid, current)| {
                    known
                        .get(pid)
                        .is_some_and(|identity| current.same_process(identity))
                })
                .map(|(_, current)| current)
                .collect(),
        },
        Err(error) => CleanupInventory::Unavailable {
            recorded,
            error: error.category().to_owned(),
        },
    }
}

#[derive(Serialize)]
pub struct ProcessSignal {
    pid: i32,
    signal: i32,
}
#[derive(Serialize)]
pub struct CleanupFailure {
    stage: Option<&'static str>,
    pid: Option<i32>,
    error: String,
}
#[derive(Serialize)]
#[serde(untagged)]
pub enum RemainingIdentity {
    Observed(ProcessIdentity),
    Unverified(&'static str),
}
#[derive(Serialize)]
pub struct CleanupResult {
    pub exit: Option<i32>,
    pub signals: Vec<ProcessSignal>,
    pub errors: Vec<CleanupFailure>,
    pub remaining: BTreeMap<i32, RemainingIdentity>,
}

fn remaining_records(
    known: &BTreeMap<i32, ProcessIdentity>,
) -> DriverResult<BTreeMap<i32, RemainingIdentity>> {
    Ok(process_table()?
        .into_iter()
        .filter(|(pid, identity)| {
            known
                .get(pid)
                .is_some_and(|recorded| identity.same_process(recorded))
        })
        .map(|(pid, identity)| (pid, RemainingIdentity::Observed(identity)))
        .collect())
}
fn unverified_inventory(
    child: &Child,
    known: &BTreeMap<i32, ProcessIdentity>,
    message: &'static str,
) -> DriverResult<BTreeMap<i32, RemainingIdentity>> {
    let root = i32::try_from(child.id()).map_err(|_| DriverError::Runtime("Host PID overflow"))?;
    let mut records: BTreeMap<_, _> = known
        .keys()
        .map(|pid| (*pid, RemainingIdentity::Unverified(message)))
        .collect();
    records.insert(root, RemainingIdentity::Unverified(message));
    Ok(records)
}
fn failure(stage: Option<&'static str>, pid: Option<i32>, error: &DriverError) -> CleanupFailure {
    CleanupFailure {
        stage,
        pid,
        error: error.category().to_owned(),
    }
}
fn signal_if_matching(
    identity: &ProcessIdentity,
    signal: rustix::process::Signal,
) -> DriverResult<bool> {
    if !lookup_identity(identity.pid)?.is_some_and(|current| current.same_process(identity)) {
        return Ok(false);
    }
    let pid = rustix::process::Pid::from_raw(identity.pid)
        .ok_or(DriverError::Invalid("invalid owned PID"))?;
    match rustix::process::kill_process(pid, signal) {
        Ok(()) => Ok(true),
        Err(rustix::io::Errno::SRCH) => Ok(false),
        Err(error) => Err(DriverError::Io(std::io::Error::from(error))),
    }
}

pub fn stop_owned(
    child: &mut Child,
    known: &mut BTreeMap<i32, ProcessIdentity>,
) -> DriverResult<CleanupResult> {
    let root = i32::try_from(child.id()).map_err(|_| DriverError::Runtime("Host PID overflow"))?;
    let mut errors = Vec::new();
    let mut signals = Vec::new();
    if let Err(error) = observe_owned(root, known) {
        errors.push(failure(Some("observe"), None, &error));
    }
    for (signal, seconds, number) in [
        (rustix::process::Signal::TERM, 45, 15),
        (rustix::process::Signal::KILL, 5, 9),
    ] {
        if let Err(error) = process_table() {
            errors.push(failure(Some("freshIdentity"), None, &error));
            return Ok(CleanupResult {
                exit: child.try_wait()?.map(exit_code),
                signals,
                errors,
                remaining: unverified_inventory(
                    child,
                    known,
                    "unverified; access required before signalling",
                )?,
            });
        }
        let ordered = known.get(&root).into_iter().chain(
            known
                .iter()
                .filter(|(pid, _)| **pid != root)
                .map(|(_, identity)| identity),
        );
        for identity in ordered {
            match signal_if_matching(identity, signal) {
                Ok(true) => signals.push(ProcessSignal {
                    pid: identity.pid,
                    signal: number,
                }),
                Ok(false) => {}
                Err(error) => errors.push(failure(None, Some(identity.pid), &error)),
            }
        }
        if wait_for_child(child, Duration::from_secs(seconds))?.is_none() {
            continue;
        }
        let mut remaining: BTreeMap<_, _> = known
            .iter()
            .map(|(pid, identity)| (*pid, RemainingIdentity::Observed(identity.clone())))
            .collect();
        let until = Instant::now() + Duration::from_secs(5);
        while Instant::now() < until {
            match remaining_records(known) {
                Ok(observed) => remaining = observed,
                Err(error) => {
                    errors.push(failure(Some("settlementObservation"), None, &error));
                    break;
                }
            }
            if remaining.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        if remaining.is_empty() {
            break;
        }
    }
    let remaining = match remaining_records(known) {
        Ok(records) => records,
        Err(error) => {
            errors.push(failure(Some("finalObservation"), None, &error));
            unverified_inventory(child, known, "unverified; final observation failed")?
        }
    };
    Ok(CleanupResult {
        exit: child.try_wait()?.map(exit_code),
        signals,
        errors,
        remaining,
    })
}

fn cleanup_owned(
    child: &mut Child,
    known: &mut BTreeMap<i32, ProcessIdentity>,
    evidence: &Path,
) -> DriverResult<()> {
    let root = i32::try_from(child.id()).map_err(|_| DriverError::Runtime("Host PID overflow"))?;
    let snapshot = match observe_owned(root, known) {
        Ok(()) => capture_cleanup_inventory(known, process_table),
        Err(error) => CleanupInventory::Unavailable {
            recorded: known.values().cloned().collect(),
            error: error.category().to_owned(),
        },
    };
    let saved = save_record(&evidence.join("host-before-fallback.json"), &snapshot).is_ok();
    if let Some(identity) = known.get(&root)
        && child.try_wait()?.is_none()
        && signal_if_matching(identity, rustix::process::Signal::TERM).is_ok()
    {
        let _ = wait_for_child(child, Duration::from_secs(45));
    }
    let result = stop_owned(child, known)?;
    save_record(&evidence.join("host-settlement.json"), &result)?;
    if !result.remaining.is_empty() || !result.errors.is_empty() {
        return Err(DriverError::Runtime(
            "owned cleanup incomplete; inspect settlement",
        ));
    }
    if !saved || matches!(snapshot, CleanupInventory::Unavailable { .. }) {
        return Err(DriverError::Runtime(
            "cleanup evidence unavailable; owned cleanup was still attempted",
        ));
    }
    Ok(())
}

pub fn with_owned_child<TValue>(
    child: &mut Child,
    evidence: &Path,
    body: impl FnOnce(&mut Child, &mut BTreeMap<i32, ProcessIdentity>) -> DriverResult<TValue>,
) -> DriverResult<TValue> {
    let mut known = BTreeMap::new();
    let result = (|| {
        let root =
            i32::try_from(child.id()).map_err(|_| DriverError::Runtime("Host PID overflow"))?;
        observe_owned(root, &mut known)?;
        let identity = known
            .get(&root)
            .ok_or(DriverError::Runtime("owned Host identity unavailable"))?;
        save_record(&evidence.join("host-started.json"), identity)?;
        body(child, &mut known)
    })();
    cleanup_owned(child, &mut known, evidence)?;
    result
}
