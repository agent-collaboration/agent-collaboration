//! The provisioned sign-in stamp is central's sole durable organization identity.
use central_storage::{Statement, StoreFailure};
use fleet_identity_protocol::OrganizationId;
use fleet_runtime_ports::CentralStore;
use fleet_runtime_ports::stored_values::string;
use fleet_runtime_ports::workspace_decode_error::corrupt;
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CentralOrganizationIdentity {
    Unprovisioned,
    Stamped { organization_id: OrganizationId },
}
/// Reads the row verified by provisioning; alarms do not verify its signature again.
pub async fn read_central_organization(
    host: &impl CentralStore,
) -> Result<CentralOrganizationIdentity, StoreFailure> {
    let rows = host
        .read(Statement {
            sql: "SELECT id,organization_statement FROM organization_identity ORDER BY id LIMIT 2",
            args: vec![],
        })
        .await?;
    if rows.rows.len() > 1 {
        return Err(corrupt("organization_identity.id"));
    }
    let Some(row) = rows.rows.first() else {
        return Ok(CentralOrganizationIdentity::Unprovisioned);
    };
    let organization_id = OrganizationId::try_from(string(&rows, row, "id")?.to_owned())
        .map_err(|_| corrupt("organization_identity.id"))?;
    if string(&rows, row, "organization_statement")?.is_empty() {
        return Err(corrupt("organization_identity.organization_statement"));
    }
    Ok(CentralOrganizationIdentity::Stamped { organization_id })
}
/// Prepares a cold alarm without fabricating an identity for an unprovisioned store.
pub async fn prepare_central_alarm(
    host: &impl CentralStore,
) -> Result<CentralOrganizationIdentity, StoreFailure> {
    let identity = read_central_organization(host).await?;
    if identity == CentralOrganizationIdentity::Unprovisioned {
        fleet_contracts::central_mail_rows::applied(
            host.run_batch(central_storage::StatementBatch {
                statements: vec![],
                alarm: fleet_engine::alarm_update(),
            })
            .await?,
        )?;
    }
    Ok(identity)
}
