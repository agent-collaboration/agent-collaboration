//! The provisioned sign-in stamp is central's sole durable organization identity.
use crate::{ObjectHost, workspace_decode_error::corrupt, workspace_row_codec::string};
use central_storage::{Statement, StoreFailure};
use fleet_identity_protocol::OrganizationId;
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CentralOrganizationIdentity {
    Unprovisioned,
    Stamped { organization_id: OrganizationId },
}
/// Reads the row verified by provisioning; alarms do not verify its signature again.
pub async fn read_central_organization(
    host: &impl ObjectHost,
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
    host: &impl ObjectHost,
) -> Result<CentralOrganizationIdentity, StoreFailure> {
    let identity = read_central_organization(host).await?;
    if identity == CentralOrganizationIdentity::Unprovisioned {
        crate::central_mail_store::applied(
            host.run_batch(central_storage::StatementBatch {
                statements: vec![],
                alarm: crate::central_mail_store::alarm_update(),
            })
            .await?,
        )?;
    }
    Ok(identity)
}
