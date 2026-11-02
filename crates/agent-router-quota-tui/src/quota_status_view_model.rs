use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

pub type QuotaStatusViewModelLoader = Arc<dyn Fn() -> QuotaStatusViewModelLoadFuture + Send + Sync>;
pub type QuotaStatusViewModelLoadFuture =
    Pin<Box<dyn Future<Output = Option<QuotaStatusViewModel>> + Send>>;

use codex_router_quota_service::quota_display_model::QuotaStatusViewModel;
