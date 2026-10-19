//! Classify the supported public rows before validating job-only fields.
use super::{NativeClaudeBackgroundJob, NativeClaudeJobInventory, NativeClaudeJobReadError};
use collaboration_protocol::{
    ClaudeCodeInteractiveStatus, NativeClaudeBackgroundState, NativeClaudeJobHandle,
    ProviderWorkingDirectory, UuidIdentity,
};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PublicJobRow {
    id: NativeClaudeJobHandle,
    state: String,
    cwd: ProviderWorkingDirectory,
    started_at: i64,
    session_id: Option<UuidIdentity>,
    name: Option<String>,
    status: Option<PublicNativeStatus>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PublicPeerRow {
    pid: u32,
    cwd: ProviderWorkingDirectory,
    started_at: i64,
    session_id: Option<UuidIdentity>,
    name: Option<String>,
    status: Option<PublicNativeStatus>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum PublicNativeStatus {
    Busy,
    Idle,
    Waiting,
}
impl From<PublicNativeStatus> for ClaudeCodeInteractiveStatus {
    fn from(status: PublicNativeStatus) -> Self {
        match status {
            PublicNativeStatus::Busy => Self::Busy,
            PublicNativeStatus::Idle => Self::Idle,
            PublicNativeStatus::Waiting => Self::Waiting,
        }
    }
}

fn valid_timestamp(timestamp_ms: i64) -> bool {
    // Public JSON comes from a JavaScript Date; reject values outside its range.
    (-8_640_000_000_000_000..=8_640_000_000_000_000).contains(&timestamp_ms)
}
fn valid_name(name: &Option<String>) -> bool {
    name.as_ref()
        .is_none_or(|name| name.len() <= 4096 && !name.contains('\0'))
}

pub(super) fn decode_background_jobs(
    bytes: &[u8],
) -> Result<NativeClaudeJobInventory, NativeClaudeJobReadError> {
    use NativeClaudeJobReadError::InvalidOutput;
    let rows: Vec<Value> = serde_json::from_slice(bytes).map_err(|_| InvalidOutput)?;
    if rows.len() > 4096 {
        return Err(InvalidOutput);
    }
    let mut jobs = Vec::new();
    let mut unidentified_jobs = 0;
    let mut handles = BTreeSet::new();
    let mut sessions = BTreeSet::new();
    for row in rows {
        let fields = row.as_object().ok_or(InvalidOutput)?;
        let kind = fields
            .get("kind")
            .and_then(Value::as_str)
            .ok_or(InvalidOutput)?;
        let job_backed = match (
            kind,
            fields.contains_key("id"),
            fields.contains_key("state"),
        ) {
            ("background", true, true) => true,
            ("background" | "interactive", false, false) => false,
            _ => return Err(InvalidOutput),
        };
        if !job_backed {
            let peer: PublicPeerRow = serde_json::from_value(row).map_err(|_| InvalidOutput)?;
            if peer.pid == 0 || !valid_timestamp(peer.started_at) || !valid_name(&peer.name) {
                return Err(InvalidOutput);
            }
            // All fields were checked as peer facts. Their discovery remains the registry's job.
            let _peer_facts = (peer.cwd, peer.session_id, peer.status);
            continue;
        }
        let job: PublicJobRow = serde_json::from_value(row).map_err(|_| InvalidOutput)?;
        let state =
            NativeClaudeBackgroundState::from_vendor_state(job.state).map_err(|_| InvalidOutput)?;
        if !valid_timestamp(job.started_at)
            || !valid_name(&job.name)
            || !handles.insert(job.id.clone())
        {
            return Err(InvalidOutput);
        }
        let Some(session_id) = job.session_id else {
            unidentified_jobs += 1;
            continue;
        };
        if !sessions.insert(session_id.clone()) {
            return Err(InvalidOutput);
        }
        jobs.push(NativeClaudeBackgroundJob {
            job_handle: job.id,
            session_id: String::from(session_id)
                .try_into()
                .map_err(|_| InvalidOutput)?,
            working_directory: job.cwd,
            name: job.name,
            started_at: job.started_at.div_euclid(1000),
            state,
            status: job.status.map(Into::into),
        });
    }
    Ok(NativeClaudeJobInventory {
        jobs,
        unidentified_jobs,
    })
}
