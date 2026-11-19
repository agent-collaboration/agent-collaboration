//! The engine's view of provisioning for its own organization.
use central_storage::StoreFailure;
use std::future::Future;

pub trait OrganizationStamp: Send + Sync {
    fn organization_is_provisioned(
        &self,
    ) -> impl Future<Output = Result<bool, StoreFailure>> + Send;
}
