use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::bounded_command::{exit_code, output};
use crate::driver_error::{DriverError, DriverResult};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ProcessIdentity {
    pub pid: i32,
    pub parent: i32,
    pub uid: i64,
    pub group: i32,
    pub birth: String,
    pub executable: String,
}
impl ProcessIdentity {
    #[must_use]
    pub fn same_process(&self, recorded: &Self) -> bool {
        self.pid == recorded.pid
            && self.uid == recorded.uid
            && self.group == recorded.group
            && self.birth == recorded.birth
            && self.executable == recorded.executable
    }
}

fn next_field<'text>(remaining: &mut &'text str) -> DriverResult<&'text str> {
    *remaining = remaining.trim_start();
    let end = remaining
        .find(char::is_whitespace)
        .unwrap_or(remaining.len());
    let (value, tail) = remaining.split_at(end);
    *remaining = tail;
    if value.is_empty() {
        return Err(DriverError::Invalid("incomplete current process identity"));
    }
    Ok(value)
}

pub fn parse_identity(line: &str, includes_parent: bool) -> DriverResult<ProcessIdentity> {
    let mut remaining = line.trim();
    let invalid = || DriverError::Invalid("incomplete current process identity");
    let pid = next_field(&mut remaining)?.parse().map_err(|_| invalid())?;
    let parent = if includes_parent {
        next_field(&mut remaining)?.parse().map_err(|_| invalid())?
    } else {
        0
    };
    let uid = next_field(&mut remaining)?.parse().map_err(|_| invalid())?;
    let group = next_field(&mut remaining)?.parse().map_err(|_| invalid())?;
    let mut birth = Vec::new();
    for _ in 0..5 {
        birth.push(next_field(&mut remaining)?);
    }
    let executable = remaining.trim_start();
    if executable.is_empty() {
        return Err(invalid());
    }
    Ok(ProcessIdentity {
        pid,
        parent,
        uid,
        group,
        birth: birth.join(" "),
        executable: executable.to_owned(),
    })
}

pub fn process_table() -> DriverResult<BTreeMap<i32, ProcessIdentity>> {
    let result = output(
        &[
            "/bin/ps".to_owned(),
            "-axo".to_owned(),
            "pid=,ppid=,uid=,pgid=,lstart=,comm=".to_owned(),
        ],
        Duration::from_secs(5),
        true,
    )?;
    let text = String::from_utf8(result.stdout)
        .map_err(|_| DriverError::Invalid("invalid process inventory UTF-8"))?;
    let mut records = BTreeMap::new();
    for line in text.lines() {
        if line.split_whitespace().count() < 10 {
            continue;
        }
        let identity = parse_identity(line, true)?;
        records.insert(identity.pid, identity);
    }
    Ok(records)
}

pub fn lookup_identity(pid: i32) -> DriverResult<Option<ProcessIdentity>> {
    let result = output(
        &[
            "/bin/ps".to_owned(),
            "-p".to_owned(),
            pid.to_string(),
            "-o".to_owned(),
            "pid=,uid=,pgid=,lstart=,comm=".to_owned(),
        ],
        Duration::from_secs(5),
        false,
    )?;
    if exit_code(result.status) == 1
        && result.stdout.iter().all(u8::is_ascii_whitespace)
        && result.stderr.iter().all(u8::is_ascii_whitespace)
    {
        return Ok(None);
    }
    if !result.status.success() {
        return Err(DriverError::Command(exit_code(result.status)));
    }
    Ok(Some(parse_identity(
        std::str::from_utf8(&result.stdout)
            .map_err(|_| DriverError::Invalid("invalid process identity UTF-8"))?,
        false,
    )?))
}

#[must_use]
pub fn owned_closure(
    root: i32,
    known: &BTreeMap<i32, ProcessIdentity>,
    observed: &BTreeMap<i32, ProcessIdentity>,
) -> BTreeSet<i32> {
    let mut owned: BTreeSet<_> = known
        .iter()
        .filter(|(pid, identity)| {
            observed
                .get(pid)
                .is_some_and(|current| current.same_process(identity))
        })
        .map(|(pid, _)| *pid)
        .collect();
    if !known.contains_key(&root) && observed.contains_key(&root) {
        owned.insert(root);
    }
    loop {
        let descendants: Vec<_> = observed
            .iter()
            .filter(|(pid, item)| {
                owned.contains(&item.parent)
                    && known
                        .get(pid)
                        .is_none_or(|recorded| item.same_process(recorded))
            })
            .map(|(pid, _)| *pid)
            .collect();
        let before = owned.len();
        owned.extend(descendants);
        if before == owned.len() {
            return owned;
        }
    }
}

pub fn observe_owned(root: i32, known: &mut BTreeMap<i32, ProcessIdentity>) -> DriverResult<()> {
    let observed = process_table()?;
    for pid in owned_closure(root, known, &observed) {
        if let Some(identity) = observed.get(&pid) {
            if identity.uid != i64::from(rustix::process::getuid().as_raw()) {
                if lookup_identity(pid)?.is_none() {
                    continue;
                }
                return Err(DriverError::Runtime("owned process UID changed"));
            }
            if known
                .get(&pid)
                .is_some_and(|recorded| !identity.same_process(recorded))
            {
                return Err(DriverError::Runtime("owned PID reused"));
            }
            known.insert(pid, identity.clone());
        }
    }
    Ok(())
}

pub fn require_protected(protected: &[ProcessIdentity]) -> DriverResult<()> {
    let observed = process_table()?;
    for identity in protected {
        if !observed
            .get(&identity.pid)
            .is_some_and(|current| current.same_process(identity))
        {
            return Err(DriverError::Runtime(
                "protected process changed or became unobservable",
            ));
        }
    }
    Ok(())
}
