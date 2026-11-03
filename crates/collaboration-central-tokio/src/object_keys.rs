//! Native object filenames and reopen decoding.
use collaboration_central_core::ObjectAddress;
use durable_steps::StoreError;
pub(crate) fn address_key(address: &ObjectAddress) -> Result<String, StoreError> {
    match address {
        ObjectAddress::Copy(project) => Ok(format!("copy-{project}")),
        ObjectAddress::Fleet => Ok("fleet".into()),
    }
}
pub(crate) fn address_from_key(key: &str) -> Result<ObjectAddress, StoreError> {
    if key == "fleet" {
        return Ok(ObjectAddress::Fleet);
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
