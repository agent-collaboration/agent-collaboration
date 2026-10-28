//! CLI-owned quota credential-store opening.
use super::{Path, QuotaCommandError};
use codex_router_quota_service::quota::quota_status_loader::{
    credential_store_availability, load_quota_status_report_with_availability_async,
};
use codex_router_quota_service::quota::quota_status_view_model::{
    CredentialStoreAvailability, QuotaStatusReport,
};
#[derive(Clone)]
pub(super) struct QuotaCredentialResources {
    credential_store:
        Option<codex_router_secret_store::runtime_credential_store::RuntimeCredentialStore>,
    availability: CredentialStoreAvailability,
}

impl QuotaCredentialResources {
    pub(super) async fn open(router_root: &Path) -> Self {
        match crate::secret_store_factory::open_cli_secret_store_async(router_root.join("secrets"))
            .await
        {
            Ok(credential_store) => Self::from_opened_store(credential_store),
            Err(_error) => Self {
                credential_store: None,
                availability: CredentialStoreAvailability::Unavailable,
            },
        }
    }

    pub(super) fn from_opened_store(
        credential_store: codex_router_secret_store::runtime_credential_store::RuntimeCredentialStore,
    ) -> Self {
        let availability = credential_store_availability(&credential_store);
        Self {
            credential_store: Some(credential_store),
            availability,
        }
    }

    pub(super) fn credential_store(
        &self,
    ) -> Option<codex_router_secret_store::runtime_credential_store::RuntimeCredentialStore> {
        self.credential_store.clone()
    }

    pub(super) fn availability(&self) -> CredentialStoreAvailability {
        self.availability.clone()
    }
}

pub(super) async fn load_quota_status_report_async(
    router_root: &Path,
    all_limits: bool,
    now_unix_seconds: u64,
    unicode_bars: bool,
) -> Result<QuotaStatusReport, QuotaCommandError> {
    let credential_resources = QuotaCredentialResources::open(router_root).await;
    load_quota_status_report_with_availability_async(
        router_root,
        all_limits,
        now_unix_seconds,
        unicode_bars,
        credential_resources.availability(),
    )
    .await
    .map_err(Into::into)
}
