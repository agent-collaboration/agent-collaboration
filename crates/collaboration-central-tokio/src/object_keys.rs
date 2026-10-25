//! Native object filenames and reopen decoding.
use collaboration_central_core::ObjectAddress;
use durable_steps::StoreError;
pub(crate) fn address_key(address: &ObjectAddress) -> Result<String, StoreError> {
    match address {
        ObjectAddress::Registry => Ok("registry".into()),
        ObjectAddress::Connection(installation) => {
            Ok(format!("connection-{}", installation.as_str()))
        }
        ObjectAddress::Copy(project) => Ok(format!("copy-{project}")),
        ObjectAddress::Mailbox(installation) => Ok(format!("mailbox-{}", installation.as_str())),
        ObjectAddress::Fleet => Ok("fleet".into()),
    }
}
pub(crate) fn address_from_key(key: &str) -> Result<ObjectAddress, StoreError> {
    if key == "fleet" {
        return Ok(ObjectAddress::Fleet);
    }
    if key == "registry" {
        return Ok(ObjectAddress::Registry);
    }
    if let Some(service) = key.strip_prefix("connection-") {
        return Ok(ObjectAddress::Connection(
            message_board::ServiceId::try_from(service.to_owned()).map_err(StoreError::backend)?,
        ));
    }
    if let Some(project) = key.strip_prefix("copy-") {
        return Ok(ObjectAddress::Copy(
            project.to_owned().try_into().map_err(StoreError::backend)?,
        ));
    }
    if let Some(service) = key.strip_prefix("mailbox-") {
        return Ok(ObjectAddress::Mailbox(
            message_board::ServiceId::try_from(service.to_owned()).map_err(StoreError::backend)?,
        ));
    }
    Err(StoreError::Aborted {
        reason: "unrecognized native object filename".into(),
    })
}
