//! Disabled-account deletion through the same non-migrating writer as account disable.

use super::account_store::parse_account_row;
use super::{AccountRecord, AccountStatus, AsyncWeeklyQuotaFloorMutationStore, StateStoreError};

/// Failures specific to disabled-account removal.
#[derive(Debug, thiserror::Error)]
pub enum AccountRemovalError {
    /// The owner must disable this account first.
    #[error(
        "account '{account_label}' is enabled; disable it first with account disable --account '{account_label}'"
    )]
    RequiresDisabled { account_label: String },
    /// The selected account no longer exists.
    #[error("account removal target was not found")]
    AccountNotFound,
    /// Login or another owner operation changed the selected target.
    #[error("account removal target changed; retry the command")]
    TargetChanged,
    /// Another writer currently owns the database.
    #[error("account removal database is busy; retry the command")]
    DatabaseBusy,
    /// The existing state boundary refused the operation.
    #[error(transparent)]
    State(#[from] StateStoreError),
}
use super::{is_busy_or_locked_sqlx_error, sqlx_error};

/// Relational cleanup held until credential deletion succeeds.
///
/// Dropping this value rolls back the cleanup. The caller must hold the existing
/// per-account credential lock before beginning, through credential deletion and commit.
pub struct PendingAccountRemoval {
    transaction: sqlx::Transaction<'static, sqlx::Sqlite>,
}

impl PendingAccountRemoval {
    /// Commits only after the caller has successfully removed the account's credentials.
    pub async fn commit(self) -> Result<(), AccountRemovalError> {
        self.transaction.commit().await.map_err(sqlx_error)?;
        Ok(())
    }
}

impl AsyncWeeklyQuotaFloorMutationStore {
    /// Rechecks the disabled target under a write transaction and stages all account cleanup.
    pub async fn begin_disabled_account_removal(
        &self,
        expected_account: &AccountRecord,
    ) -> Result<PendingAccountRemoval, AccountRemovalError> {
        let mut transaction = self
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(|error| {
                if is_busy_or_locked_sqlx_error(&error) {
                    AccountRemovalError::DatabaseBusy
                } else {
                    AccountRemovalError::State(sqlx_error(error))
                }
            })?;
        let account_id = expected_account.account_id().as_str();
        let row = sqlx::query!(
            "SELECT account_id, label, status, active_credential_generation, provider
               FROM accounts
              WHERE account_id = ?1",
            account_id,
        )
        .fetch_optional(&mut *transaction)
        .await
        .map_err(sqlx_error)?
        .ok_or(AccountRemovalError::AccountNotFound)?;
        let account = parse_account_row(
            row.account_id,
            row.label,
            row.status,
            row.active_credential_generation,
            row.provider,
        )?;
        if account.label() != expected_account.label()
            || account.provider() != expected_account.provider()
        {
            return Err(AccountRemovalError::TargetChanged);
        }
        if account.status() != AccountStatus::Disabled {
            return Err(AccountRemovalError::RequiresDisabled {
                account_label: account.label().to_owned(),
            });
        }
        if account.active_credential_generation() != expected_account.active_credential_generation()
        {
            return Err(AccountRemovalError::TargetChanged);
        }

        // Older tables intentionally have no FK. Delete their account-owned rows
        // explicitly; keep this in one transaction with the cascading newer tables.
        for statement in [
            "DELETE FROM quota_snapshots WHERE account_id = ?1",
            "DELETE FROM affinity_pins WHERE account_id = ?1",
            "DELETE FROM selector_quota_windows WHERE account_id = ?1",
            "DELETE FROM quota_refresh_status WHERE account_id = ?1",
            "DELETE FROM previous_response_affinity_owners WHERE account_id = ?1",
            "DELETE FROM quota_history_observations WHERE account_id = ?1",
            "DELETE FROM active_client_leases WHERE account_id = ?1",
            "DELETE FROM route_band_account_states WHERE account_id = ?1",
            "DELETE FROM active_session_events WHERE account_id = ?1",
            "DELETE FROM active_session_rollups WHERE account_id = ?1",
            "DELETE FROM account_routing_policies WHERE account_id = ?1",
            "DELETE FROM session_account_affinities WHERE account_id = ?1",
            "DELETE FROM credential_maintenance WHERE account_id = ?1",
            "DELETE FROM accounts WHERE account_id = ?1",
        ] {
            sqlx::query(statement)
                .bind(account_id)
                .execute(&mut *transaction)
                .await
                .map_err(sqlx_error)?;
        }
        Ok(PendingAccountRemoval { transaction })
    }
}
