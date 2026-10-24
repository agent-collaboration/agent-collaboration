//! Stable process identity and owned-process inventories for the Lead-operated
//! `codex-router serve`/`host` lifetimes. Observation only: nothing here signals.
//!
//! The owner of a process is ps(1) `ruid`, the real user ID. ps `uid` is the
//! effective ID, which a setuid descendant raises without changing its owner.
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf, process::Command};

/// The `ps -o` keywords the observers request, in field order.
pub(crate) const TABLE_FIELDS: &str = "pid=,ppid=,ruid=,pgid=,lstart=,comm=";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ProcessIdentity {
    pub(crate) pid: u32,
    pub(crate) parent: u32,
    pub(crate) real_uid: u32,
    pub(crate) group: u32,
    pub(crate) birth: String,
    pub(crate) executable: PathBuf,
}

impl ProcessIdentity {
    /// Stable identity: everything but the mutable parent.
    pub(crate) fn same_process(&self, other: &Self) -> bool {
        self.pid == other.pid
            && self.real_uid == other.real_uid
            && self.group == other.group
            && self.birth == other.birth
            && self.executable == other.executable
    }
}

/// A `uid_t`, which macOS ps prints as a signed number: `nobody` (4294967294) is `-2`.
fn parse_user_id(word: &str) -> Option<u32> {
    word.parse::<u32>().ok().or_else(|| {
        word.parse::<i32>()
            .ok()
            .map(|signed| u32::from_ne_bytes(signed.to_ne_bytes()))
    })
}

/// Parse `ps -axo pid=,ppid=,ruid=,pgid=,lstart=,comm=` output. `lstart` is five
/// words; the command is the remainder and may contain spaces. Malformed rows fail.
pub(crate) fn parse_process_table(output: &str) -> Result<BTreeMap<u32, ProcessIdentity>, String> {
    let mut table = BTreeMap::new();
    for line in output.lines().filter(|line| !line.trim().is_empty()) {
        let mut words = line.split_whitespace();
        let mut number = |name: &str, parse: fn(&str) -> Option<u32>| -> Result<u32, String> {
            words
                .next()
                .and_then(parse)
                .ok_or_else(|| format!("process row lacks {name}: {line:?}"))
        };
        let plain = |word: &str| word.parse().ok();
        let (pid, parent, real_uid, group) = (
            number("pid", plain)?,
            number("ppid", plain)?,
            number("ruid", parse_user_id)?,
            number("pgid", plain)?,
        );
        let birth: Vec<&str> = words.by_ref().take(5).collect();
        let executable: Vec<&str> = words.collect();
        if birth.len() != 5 || executable.is_empty() {
            return Err(format!("incomplete process row: {line:?}"));
        }
        table.insert(
            pid,
            ProcessIdentity {
                pid,
                parent,
                real_uid,
                group,
                birth: birth.join(" "),
                executable: PathBuf::from(executable.join(" ")),
            },
        );
    }
    Ok(table)
}

/// The live process table, owner by real UID.
pub(crate) fn observe_process_table() -> Result<BTreeMap<u32, ProcessIdentity>, String> {
    let output = Command::new("/bin/ps")
        .args(["-axo", TABLE_FIELDS])
        .output()
        .map_err(|error| format!("ps could not run: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "ps failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    parse_process_table(&String::from_utf8_lossy(&output.stdout))
}

/// Parse `ps -axww -o pid=,args=`: each live PID with its full argument line.
pub(crate) fn parse_process_arguments(output: &str) -> Result<BTreeMap<u32, String>, String> {
    output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let trimmed = line.trim_start();
            let (pid, arguments) = trimmed
                .split_once(char::is_whitespace)
                .unwrap_or((trimmed, ""));
            let pid = pid
                .parse()
                .map_err(|_| format!("process argument row lacks pid: {line:?}"))?;
            Ok((pid, arguments.trim().to_owned()))
        })
        .collect()
}

/// Full argument lines of live processes; used only to bind a declared path to a
/// process, never retained as evidence.
pub(crate) fn observe_process_arguments() -> Result<BTreeMap<u32, String>, String> {
    let output = Command::new("/bin/ps")
        .args(["-axww", "-o", "pid=,args="])
        .output()
        .map_err(|error| format!("ps could not run: {error}"))?;
    if !output.status.success() {
        return Err("ps argument listing failed".to_owned());
    }
    parse_process_arguments(&String::from_utf8_lossy(&output.stdout))
}

/// The real UID of this verifier process (getuid), read through ps for our own PID.
pub(crate) fn own_real_uid() -> Result<u32, String> {
    let table = observe_process_table()?;
    table
        .get(&std::process::id())
        .map(|identity| identity.real_uid)
        .ok_or_else(|| "own process absent from ps".to_owned())
}

/// Processes owned by this run: the recorded identities still present unchanged, plus
/// descendants acquired only through a matching parent and only with the owner's real
/// UID. A known PID whose identity changed is never acquired (PID reuse).
pub(crate) fn owned_closure(
    root_pid: u32,
    owner_uid: u32,
    known: &BTreeMap<u32, ProcessIdentity>,
    observed: &BTreeMap<u32, ProcessIdentity>,
) -> Result<BTreeMap<u32, ProcessIdentity>, String> {
    let mut owned: BTreeMap<u32, ProcessIdentity> = known
        .iter()
        .filter_map(|(pid, recorded)| {
            observed
                .get(pid)
                .filter(|current| current.same_process(recorded))
                .map(|current| (*pid, current.clone()))
        })
        .collect();
    if !known.contains_key(&root_pid)
        && let Some(root) = observed.get(&root_pid)
    {
        owned.insert(root_pid, root.clone());
    }
    loop {
        let descendants: Vec<ProcessIdentity> = observed
            .values()
            .filter(|item| owned.contains_key(&item.parent) && !owned.contains_key(&item.pid))
            .filter(|item| {
                known
                    .get(&item.pid)
                    .is_none_or(|recorded| item.same_process(recorded))
            })
            .cloned()
            .collect();
        if descendants.is_empty() {
            break;
        }
        for item in descendants {
            if item.real_uid != owner_uid {
                return Err(format!(
                    "owned process {} has a foreign real owner {}",
                    item.pid, item.real_uid
                ));
            }
            owned.insert(item.pid, item);
        }
    }
    if let Some(foreign) = owned.values().find(|item| item.real_uid != owner_uid) {
        return Err(format!(
            "owned process {} has a foreign real owner {}",
            foreign.pid, foreign.real_uid
        ));
    }
    Ok(owned)
}

/// Recorded identities still present unchanged; a disappeared or reused PID is not owned.
pub(crate) fn remaining_owned(
    known: &BTreeMap<u32, ProcessIdentity>,
    observed: &BTreeMap<u32, ProcessIdentity>,
) -> BTreeMap<u32, ProcessIdentity> {
    known
        .iter()
        .filter(|(pid, recorded)| {
            observed
                .get(pid)
                .is_some_and(|current| current.same_process(recorded))
        })
        .map(|(pid, recorded)| (*pid, recorded.clone()))
        .collect()
}

/// Whether the recorded Router/Host process is the live one, by stable identity.
pub(crate) fn recorded_process_live(
    recorded: &crate::verification_models::RecordedProcess,
    observed: &BTreeMap<u32, ProcessIdentity>,
) -> bool {
    observed.get(&recorded.pid).is_some_and(|current| {
        current.real_uid == recorded.real_uid
            && current.group == recorded.group
            && current.birth == recorded.birth
            && current.executable == recorded.executable
    })
}
