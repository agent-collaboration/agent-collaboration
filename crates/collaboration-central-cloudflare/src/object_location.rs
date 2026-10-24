//! Physical namespace and Durable Object name for a core address.
use collaboration_central_core::ObjectAddress;
pub(crate) fn location(address: &ObjectAddress) -> (&'static str, String) {
    match address {
        ObjectAddress::Registry => ("LEASE_REGISTRY", "registry".into()),
        ObjectAddress::Connection(service) => ("CONNECTIONS", service.as_str().to_owned()),
        ObjectAddress::Copy(project) => ("PROJECT_COPIES", project.to_string()),
        ObjectAddress::Fleet => ("FLEET", "fleet".into()),
        ObjectAddress::Mailbox(service) => ("MAILBOXES", service.as_str().to_owned()),
    }
}
