//! Native object filenames and reopen decoding under explicit deployment policy.
use collaboration_central_core::{FleetLocationPolicy, ObjectAddress};
use durable_steps::StoreError;
pub(crate) fn address_key(
    address: &ObjectAddress,
    locations: &FleetLocationPolicy,
) -> Result<String, StoreError> {
    match address {
        ObjectAddress::Copy(project) => Ok(format!("copy-{project}")),
        ObjectAddress::Fleet { organization } => Ok(locations.native_key(organization)),
    }
}
pub(crate) fn address_from_key(
    key: &str,
    locations: &FleetLocationPolicy,
) -> Result<ObjectAddress, StoreError> {
    if key == "fleet" || key.starts_with("fleet-") {
        let organization = locations
            .organization_from_native_key(key)
            .map_err(StoreError::backend)?;
        return Ok(ObjectAddress::Fleet { organization });
    }
    if let Some(project) = key.strip_prefix("copy-") {
        return Ok(ObjectAddress::Copy(
            project.to_owned().try_into().map_err(StoreError::backend)?,
        ));
    }
    Err(StoreError::Aborted {
        reason: "unrecognized native object filename".into(),
    })
}
