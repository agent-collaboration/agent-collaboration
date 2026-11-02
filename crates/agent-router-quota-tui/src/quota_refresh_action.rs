//! Quota screen's refresh boundary; the CLI supplies the existing service interaction.
use codex_router_core::ids::AccountId;
use std::{future::Future, pin::Pin, sync::Arc};

pub type QuotaRefresher = Arc<dyn Fn() -> QuotaRefreshFuture + Send + Sync>;
pub type QuotaRefreshFuture = Pin<Box<dyn Future<Output = QuotaRefreshResult> + Send>>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuotaRefreshFailure {
    pub account_id: AccountId,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QuotaRefreshResult {
    Refreshed,
    Partial {
        failed: Vec<QuotaRefreshFailure>,
    },
    Failed {
        failed: Vec<QuotaRefreshFailure>,
        overall: Option<String>,
    },
}
