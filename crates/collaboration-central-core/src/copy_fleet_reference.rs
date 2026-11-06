//! Resolve a Copy's central-private reference without trusting replicated data.
use durable_steps::BoxFuture;
use fleet_identity_protocol::OrganizationId;
use fleet_sync_protocol::DatabaseId;
use std::{collections::BTreeMap, sync::Mutex};

/// Hosts resolve their private opaque reference into the admitting Fleet's
/// logical organization. Persistent adapters remain gated on SQL approval.
pub trait CopyFleetReferencePort: Send + Sync + 'static {
    fn bind(
        &self,
        project: DatabaseId,
        organization: OrganizationId,
    ) -> BoxFuture<'_, Result<(), CopyFleetReferenceRefusal>>;
    fn resolve(
        &self,
        project: DatabaseId,
    ) -> BoxFuture<'_, Result<OrganizationId, CopyFleetReferenceRefusal>>;
}

/// Explicit B2a stand-in only. It is not durable across host-process restart.
/// Future native/CF adapters store only the approved opaque reference, not an
/// organization field in replicated project data or new ownership authority.
#[derive(Default)]
pub struct InMemoryCopyFleetReferences {
    references: Mutex<BTreeMap<DatabaseId, OrganizationId>>,
}

impl CopyFleetReferencePort for InMemoryCopyFleetReferences {
    fn bind(
        &self,
        project: DatabaseId,
        organization: OrganizationId,
    ) -> BoxFuture<'_, Result<(), CopyFleetReferenceRefusal>> {
        Box::pin(async move {
            let mut references = self
                .references
                .lock()
                .map_err(|_| CopyFleetReferenceRefusal::Unavailable)?;
            match references.entry(project) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(organization);
                    Ok(())
                }
                std::collections::btree_map::Entry::Occupied(entry)
                    if entry.get() == &organization =>
                {
                    Ok(())
                }
                std::collections::btree_map::Entry::Occupied(_) => {
                    Err(CopyFleetReferenceRefusal::OrganizationMismatch)
                }
            }
        })
    }

    fn resolve(
        &self,
        project: DatabaseId,
    ) -> BoxFuture<'_, Result<OrganizationId, CopyFleetReferenceRefusal>> {
        Box::pin(async move {
            self.references
                .lock()
                .map_err(|_| CopyFleetReferenceRefusal::Unavailable)?
                .get(&project)
                .cloned()
                .ok_or(CopyFleetReferenceRefusal::Missing)
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum CopyFleetReferenceRefusal {
    #[error("Copy has no private Fleet reference")]
    Missing,
    #[error("Copy is already bound to another Fleet")]
    OrganizationMismatch,
    #[error("Copy's private Fleet reference is unavailable")]
    Unavailable,
}

#[cfg(test)]
#[path = "copy_fleet_reference_test.rs"]
mod tests;
