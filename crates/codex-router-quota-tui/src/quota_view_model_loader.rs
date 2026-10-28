//! Reload the quota view model from the injected domain resources.
use crate::QuotaStatusViewModelLoader;
#[cfg(feature = "quota-reset-test-harness")]
use codex_router_quota_service::quota::quota_status_metrics::record_storyboard_quota_error;
use codex_router_quota_service::quota::{
    CredentialStoreAvailability, current_unix_seconds,
    load_quota_status_report_with_availability_async, quota_status_view_model,
};
use std::path::PathBuf;
use std::sync::Arc;
pub fn quota_status_view_model_loader(
    router_root: PathBuf,
    all_limits: bool,
    unicode_bars: bool,
    width: usize,
    credential_store_availability: CredentialStoreAvailability,
) -> QuotaStatusViewModelLoader {
    Arc::new(move || {
        let router_root = router_root.clone();
        let credential_store_availability = credential_store_availability.clone();
        Box::pin(async move {
            let report = load_quota_status_report_with_availability_async(
                &router_root,
                all_limits,
                current_unix_seconds(),
                unicode_bars,
                credential_store_availability,
            )
            .await;
            let report = match report {
                Ok(report) => report,
                Err(_error) => {
                    #[cfg(feature = "quota-reset-test-harness")]
                    record_storyboard_quota_error("report_reload", &_error);
                    return None;
                }
            };
            Some(quota_status_view_model(&report, report.rows(), width))
        })
    })
}
