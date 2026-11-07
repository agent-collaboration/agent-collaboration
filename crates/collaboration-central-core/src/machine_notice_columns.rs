//! Translate project control notices to their typed storage columns.
use crate::fleet_record_codec::{check_private_id, corrupt, required};
use board_sync_protocol::BoardNotice;
use durable_steps::StoreError;
use fleet_sync_protocol::Epoch;

pub(super) struct NoticeColumns {
    pub notice: BoardNotice,
    pub kind: &'static str,
    pub project: Option<String>,
    pub epoch: Option<String>,
}

impl NoticeColumns {
    pub fn from_notice(notice: BoardNotice) -> Self {
        let (kind, project, epoch) = match &notice {
            BoardNotice::ProjectListChanged {} => ("project_list_changed", None, None),
            BoardNotice::LeaseGranted { project, epoch } => (
                "lease_granted",
                Some(project.as_str().to_owned()),
                Some(epoch.value().to_string()),
            ),
            BoardNotice::LeaseSuperseded { project, epoch } => (
                "lease_superseded",
                Some(project.as_str().to_owned()),
                Some(epoch.value().to_string()),
            ),
        };
        Self {
            notice,
            kind,
            project,
            epoch,
        }
    }
}

pub(super) fn decode_notice(
    kind: Option<String>,
    project: Option<String>,
    epoch: Option<String>,
) -> Result<BoardNotice, StoreError> {
    match (
        required(kind, "machine_control_project_notices.kind")?.as_str(),
        project,
        epoch,
    ) {
        ("project_list_changed", None, None) => Ok(BoardNotice::ProjectListChanged {}),
        (tag @ ("lease_granted" | "lease_superseded"), Some(project), Some(epoch)) => {
            check_private_id(&project, "machine_control_project_notices.project_id")?;
            let project = project
                .try_into()
                .map_err(|error| corrupt("machine_control_project_notices.project_id", error))?;
            let epoch = Epoch::new(canonical_counter(&epoch)?)
                .map_err(|error| corrupt("machine_control_project_notices.epoch", error))?;
            Ok(if tag == "lease_granted" {
                BoardNotice::LeaseGranted { project, epoch }
            } else {
                BoardNotice::LeaseSuperseded { project, epoch }
            })
        }
        _ => Err(corrupt(
            "machine_control_project_notices.kind",
            "unknown kind or mismatched variant columns",
        )),
    }
}

fn canonical_counter(value: &str) -> Result<u64, StoreError> {
    let counter = value
        .parse::<u64>()
        .map_err(|error| corrupt("machine_control_project_notices.counter", error))?;
    if counter.to_string() != value || i64::try_from(counter).is_err() {
        return Err(corrupt(
            "machine_control_project_notices.counter",
            "noncanonical SQLite integer",
        ));
    }
    Ok(counter)
}

#[cfg(test)]
#[path = "machine_notice_columns_test.rs"]
mod tests;
