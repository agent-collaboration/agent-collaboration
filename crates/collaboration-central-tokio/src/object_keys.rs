//! Native object filenames and reopen decoding.
use collaboration_central_core::ObjectAddress;
use durable_steps::StoreError;
pub(crate) fn address_key(address: &ObjectAddress) -> Result<String, StoreError> {
    match address {
        ObjectAddress::Fleet => Ok("fleet".into()),
    }
}
pub(crate) fn address_from_key(key: &str) -> Result<ObjectAddress, StoreError> {
    if key == "fleet" {
        return Ok(ObjectAddress::Fleet);
    }
    Err(StoreError::Aborted {
        reason: "unrecognized native object filename".into(),
    })
}
