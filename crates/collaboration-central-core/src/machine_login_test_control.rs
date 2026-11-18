//! Closed test-support dispatch to the real private revocation decision.
use crate::WorkspaceRuntime;
use crate::identity_contract::{EndedMachineClient, PersonId};
use crate::organization_authority::machine_login::{self, EnrollmentQuery};
use crate::person_request::AdminOperationId;
use central_storage::StoreFailure;
use fleet_identity_protocol::{EnrollmentId, InstallationId};
use fleet_runtime_ports::{CentralStore, WorkspaceStores};
use serde::{Deserialize, Serialize};

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
        installation: InstallationId,
        enrollment_id: EnrollmentId,
    },
}

pub async fn machine_login_control<H: CentralStore, S: WorkspaceStores>(
    runtime: &WorkspaceRuntime<H, S>,
    control: MachineLoginControl,
) -> Result<Option<EndedMachineClient>, StoreFailure> {
    let _turn = runtime.take_central_turn().await;
    let time = runtime.machine_login_time(&runtime.organization).await?;
    match control {
        MachineLoginControl::Revoke {
            person_id,
            operation_id,
            installation,
            enrollment_id,
        } => {
            let query = EnrollmentQuery {
                installation: Some(installation),
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
