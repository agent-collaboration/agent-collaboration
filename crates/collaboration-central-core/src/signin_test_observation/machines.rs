//! Fixture wire projections are built only from the owning machine row codecs.
use crate::{
    identity_contract::{
        AllowedWorkspaces, ClientId, DeviceLoginRequestId, EnrollmentRevocationReason,
    },
    machine_login_row_values::{seconds_timestamp, timestamp},
    machine_login_rows,
    organization_authority::machine_login::{MachineState, StoredEnrollmentState},
};
use central_storage::{ReadOutcome, SafeInteger, SqlValue, StoreFailure};
use fleet_identity_protocol::{EnrollmentId, InstallationId};
use fleet_runtime_ports::workspace_decode_error::corrupt;
use message_board::WorkspaceId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ObservedMachineState {
    Enrolled,
    Revoked,
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ObservedMachine {
    pub machine_id: Uuid,
    pub installation_id: InstallationId,
    pub state: ObservedMachineState,
    pub allowed_workspaces: Vec<WorkspaceId>,
    pub enrollment_generation: u64,
}

#[derive(Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ObservedApprovalState {
    Approved,
    Enrolled,
    Ended,
}

#[derive(Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ObservedApprovalEndReason {
    Lapsed,
    Owner,
    Superseded,
    Idle,
}

/// Flat fixture fields mirror the agreed columns; the row codec owns lifecycle validity.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ObservedMachineApproval {
    pub approval_id: EnrollmentId,
    pub request_id: DeviceLoginRequestId,
    pub machine_id: Uuid,
    pub state: ObservedApprovalState,
    pub end_reason: Option<ObservedApprovalEndReason>,
    pub allowed_workspaces: AllowedWorkspaces,
    pub base_generation: u64,
    pub code_expires_at: String,
    pub machine_client_id: Option<ClientId>,
    pub last_grant_at: Option<String>,
    pub ended_at: Option<String>,
    pub created_at: String,
}

// JSON is only the one-statement snapshot's transport. Preserve SQL scalar
// types, then use the same decoders as decision reads instead of revalidating
// selected fields or letting malformed unexported columns escape validation.
#[derive(Deserialize)]
#[serde(untagged)]
enum StoredScalar {
    Text(String),
    Integer(SafeInteger),
    Null,
}

#[derive(Deserialize)]
#[serde(transparent)]
pub(super) struct StoredMachineRow(BTreeMap<String, StoredScalar>);

impl StoredMachineRow {
    fn into_read_outcome(self) -> ReadOutcome {
        let (columns, row) = self
            .0
            .into_iter()
            .map(|(name, scalar)| {
                let value = match scalar {
                    StoredScalar::Text(value) => SqlValue::Text(value),
                    StoredScalar::Integer(value) => SqlValue::Integer(value),
                    StoredScalar::Null => SqlValue::Null,
                };
                (name, value)
            })
            .unzip();
        ReadOutcome {
            columns,
            rows: vec![row],
        }
    }
}

pub(super) fn decode_machine_observation(
    row: StoredMachineRow,
) -> Result<ObservedMachine, StoreFailure> {
    let rows = row.into_read_outcome();
    let row = rows
        .rows
        .first()
        .ok_or_else(|| corrupt("signin_observation.machine"))?;
    let checked = machine_login_rows::machine(&rows, row)?;
    Ok(ObservedMachine {
        machine_id: checked.id,
        installation_id: checked.installation,
        state: match checked.state {
            MachineState::Enrolled => ObservedMachineState::Enrolled,
            MachineState::Revoked => ObservedMachineState::Revoked,
        },
        allowed_workspaces: checked.allowed_workspaces,
        enrollment_generation: checked.generation.get(),
    })
}

pub(super) fn decode_approval_observation(
    row: StoredMachineRow,
) -> Result<(i64, ObservedMachineApproval), StoreFailure> {
    let rows = row.into_read_outcome();
    let row = rows
        .rows
        .first()
        .ok_or_else(|| corrupt("signin_observation.machine_approval"))?;
    let checked = machine_login_rows::enrollment(&rows, row)?;
    let (state, end_reason, machine_client_id, last_grant_at) = match checked.state {
        StoredEnrollmentState::Approved => (ObservedApprovalState::Approved, None, None, None),
        StoredEnrollmentState::Enrolled {
            client_id,
            last_grant_at,
        } => (
            ObservedApprovalState::Enrolled,
            None,
            Some(client_id),
            Some(seconds_timestamp(
                last_grant_at,
                "machine_approvals.last_grant_at",
            )?),
        ),
        StoredEnrollmentState::Lapsed => (
            ObservedApprovalState::Ended,
            Some(ObservedApprovalEndReason::Lapsed),
            None,
            None,
        ),
        StoredEnrollmentState::Revoked { client_id, reason } => (
            ObservedApprovalState::Ended,
            Some(match reason {
                EnrollmentRevocationReason::Owner {} => ObservedApprovalEndReason::Owner,
                EnrollmentRevocationReason::Superseded {} => ObservedApprovalEndReason::Superseded,
            }),
            Some(client_id),
            None,
        ),
        StoredEnrollmentState::Expired { client_id } => (
            ObservedApprovalState::Ended,
            Some(ObservedApprovalEndReason::Idle),
            Some(client_id),
            None,
        ),
    };
    // Retain the checked instant for ordering; valid RFC3339 spellings can
    // reverse lexical order even after their wire values become canonical.
    Ok((
        checked.approved_at_ms,
        ObservedMachineApproval {
            approval_id: checked.id,
            request_id: checked.request_id,
            machine_id: checked.machine_id,
            state,
            end_reason,
            allowed_workspaces: checked.allowed_workspaces,
            base_generation: checked.base_generation.get(),
            code_expires_at: seconds_timestamp(
                checked.code_expires_at,
                "machine_approvals.code_expires_at",
            )?,
            machine_client_id,
            last_grant_at,
            ended_at: checked
                .ended_at_ms
                .map(|value| timestamp(value, "machine_approvals.ended_at"))
                .transpose()?,
            created_at: timestamp(checked.approved_at_ms, "machine_approvals.created_at")?,
        },
    ))
}
