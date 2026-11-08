//! The private link's surface: provisioning, the renewal digest read, floor
//! delivery and the three admin routes, each forwarded through a port to the
//! named organization's Fleet object. The ports' transport over a host's
//! object calls lives with the object-call transport (`object_transport`).
use crate::{
    organization_admin_link::{OrganizationAdminPort, admin_link_routes},
    organization_authority::OrganizationProvisioningPort,
};
use std::sync::Arc;

/// The link router over its two ports.
pub(crate) fn link_routes<Port>(port: Port) -> axum::Router
where
    Port: OrganizationProvisioningPort + OrganizationAdminPort + Clone,
{
    crate::organization_provisioning_router(Arc::new(port.clone())).merge(admin_link_routes(port))
}

/// Whether a path belongs to the private link's surface (all of it, on any host).
#[must_use]
pub fn is_private_link_path(path: &str) -> bool {
    path.starts_with(crate::PRIVATE_LINK_PREFIX)
        || path.starts_with(crate::ORGANIZATION_ADMIN_PREFIX)
}
