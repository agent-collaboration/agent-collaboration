//! Captured account identity and the effect boundary; controls never resolve labels.
use codex_router_core::{ids::AccountId, provider::Provider};
use codex_router_quota_service::quota_display_model::QuotaStatusAccountViewModel;
use std::{future::Future, pin::Pin, sync::Arc};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuotaAccountLabel(String);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EmptyAccountLabel;

impl std::fmt::Display for EmptyAccountLabel {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("account label is empty")
    }
}
impl std::error::Error for EmptyAccountLabel {}

impl QuotaAccountLabel {
    pub fn new(value: impl Into<String>) -> Result<Self, EmptyAccountLabel> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(EmptyAccountLabel);
        }
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountActionTarget {
    pub account_id: AccountId,
    pub label: QuotaAccountLabel,
    pub provider: Provider,
    /// Used only to select a surviving neighbor after this account is removed.
    pub position: usize,
}
impl AccountActionTarget {
    pub fn from_row(
        row: &QuotaStatusAccountViewModel,
        position: usize,
    ) -> Result<Self, EmptyAccountLabel> {
        Ok(Self {
            account_id: row.account_id.clone(),
            label: QuotaAccountLabel::new(row.account.clone())?,
            provider: row.provider,
            position,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuotaAccountAction {
    Disable,
    Enable,
    Remove,
    CopyId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuotaAccountCommand {
    pub target: AccountActionTarget,
    pub action: QuotaAccountAction,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuotaAccountRefusal {
    RequiresDisabled,
    TargetChanged,
    AccountNoLongerListed,
}
impl QuotaAccountRefusal {
    pub const fn message(self) -> &'static str {
        match self {
            Self::RequiresDisabled => "Disable the account before removing it",
            Self::TargetChanged => "Account changed; reopen its Details",
            Self::AccountNoLongerListed => "Account is no longer listed",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QuotaAccountCommandResult {
    Succeeded,
    Refused { reason: QuotaAccountRefusal },
    Failed { message: String },
}

pub type QuotaAccountCommands =
    Arc<dyn Fn(QuotaAccountCommand) -> QuotaAccountCommandFuture + Send + Sync>;
pub type QuotaAccountCommandFuture =
    Pin<Box<dyn Future<Output = QuotaAccountCommandResult> + Send>>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QuotaOutcome {
    Quit,
    LogInAgain { target: AccountActionTarget },
}
