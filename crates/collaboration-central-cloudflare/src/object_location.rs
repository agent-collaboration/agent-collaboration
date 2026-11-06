//! Physical namespace and Durable Object name under explicit deployment policy.
use collaboration_central_core::{FleetLocationPolicy, ObjectAddress};
pub(crate) fn location(
    address: &ObjectAddress,
    locations: &FleetLocationPolicy,
) -> (&'static str, String) {
    match address {
        ObjectAddress::Copy(project) => ("PROJECT_COPIES", project.to_string()),
        ObjectAddress::Fleet { organization } => ("FLEET", locations.cloudflare_name(organization)),
    }
}
