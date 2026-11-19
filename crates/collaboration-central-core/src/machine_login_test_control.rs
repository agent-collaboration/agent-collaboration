//! Closed test-support dispatch to the real private revocation decision.
use crate::identity_contract::{EndedMachineClient, PersonId};
use crate::organization_authority::machine_login::{self, EnrollmentQuery};
use crate::person_request::AdminOperationId;
use crate::{ObjectHost, WorkspaceRuntime, WorkspaceStoreCalls};
use central_storage::StoreFailure;
use fleet_identity_protocol::EnrollmentId;
use serde::{Deserialize, Serialize};
use workspace_ids::MachineId;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum MachineLoginControl {
    Revoke {
        person_id: PersonId,
        operation_id: AdminOperationId,
        machine_id: MachineId,
        enrollment_id: EnrollmentId,
    },
}

pub async fn machine_login_control<H: ObjectHost, S: WorkspaceStoreCalls>(
    runtime: &WorkspaceRuntime<H, S>,
    control: MachineLoginControl,
) -> Result<Option<EndedMachineClient>, StoreFailure> {
    let _turn = runtime.take_central_turn().await;
    let time = runtime.machine_login_time(&runtime.organization).await?;
    match control {
        MachineLoginControl::Revoke {
            person_id,
            operation_id,
            machine_id,
            enrollment_id,
        } => {
            let query = EnrollmentQuery {
                machine_id: Some(machine_id),
                enrollment: Some(enrollment_id.clone()),
                ..EnrollmentQuery::default()
            };
            let facts = crate::machine_login_reads::facts(
                runtime.host.as_ref(),
                runtime.organization.clone(),
                &query,
            )
            .await?;
            if facts
                .active
                .as_ref()
                .is_none_or(|active| active.id != enrollment_id)
            {
                return Ok(None);
            }
            let decided = machine_login::revoke_machine_enrollment(
                &facts,
                &person_id,
                &operation_id,
                time,
                runtime.ids.as_ref(),
            )?;
            runtime.apply_machine_login(&facts, decided, time).await
        }
    }
}
