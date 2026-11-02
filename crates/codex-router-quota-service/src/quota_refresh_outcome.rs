//! Invocation-local refresh facts, separate from stored quota and human diagnostics.
use codex_router_core::ids::AccountId;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountRefreshOutcome {
    pub account_id: AccountId,
    pub outcome: AccountRefreshStatus,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AccountRefreshStatus {
    Refreshed,
    Failed { reason: String },
    Skipped { reason: String },
}

/// One account may have several route bands; a failed band must survive later success.
#[derive(Default)]
pub(crate) struct AccountRefreshOutcomes(Vec<AccountRefreshOutcome>);

impl AccountRefreshOutcomes {
    pub(crate) fn record_failed(&mut self, account_id: &AccountId, reason: impl Into<String>) {
        self.record(
            account_id,
            AccountRefreshStatus::Failed {
                reason: reason.into(),
            },
        );
    }

    pub(crate) fn record_skipped(&mut self, account_id: &AccountId, reason: impl Into<String>) {
        self.record(
            account_id,
            AccountRefreshStatus::Skipped {
                reason: reason.into(),
            },
        );
    }

    pub(crate) fn record_refreshed(&mut self, account_id: &AccountId) {
        self.record(account_id, AccountRefreshStatus::Refreshed);
    }

    pub(crate) fn record(&mut self, account_id: &AccountId, outcome: AccountRefreshStatus) {
        let Some(existing) = self
            .0
            .iter_mut()
            .find(|entry| &entry.account_id == account_id)
        else {
            self.0.push(AccountRefreshOutcome {
                account_id: account_id.clone(),
                outcome,
            });
            return;
        };
        match (&mut existing.outcome, outcome) {
            (
                AccountRefreshStatus::Failed { reason },
                AccountRefreshStatus::Failed { reason: next },
            ) => {
                if !reason.split("; ").any(|part| part == next) {
                    reason.push_str("; ");
                    reason.push_str(&next);
                }
            }
            (AccountRefreshStatus::Failed { .. }, _) => {}
            (_, outcome @ AccountRefreshStatus::Failed { .. }) => existing.outcome = outcome,
            (_, AccountRefreshStatus::Refreshed) => {
                existing.outcome = AccountRefreshStatus::Refreshed
            }
            (
                AccountRefreshStatus::Skipped { reason },
                AccountRefreshStatus::Skipped { reason: next },
            ) => *reason = next,
            (AccountRefreshStatus::Refreshed, AccountRefreshStatus::Skipped { .. }) => {}
        }
    }

    pub(crate) fn into_outcomes(self) -> Vec<AccountRefreshOutcome> {
        self.0
    }
}
