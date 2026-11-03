//! Fake state is created privately; all later reads use the real report and mutation owners.
use super::configuration::Scenario;
use agent_router_quota_tui::{AccountActionTarget, QuotaAccountLabel, QuotaStatusViewModelLoader};
use codex_router_core::{ids::AccountId, provider::Provider};
use codex_router_quota_service::{
    quota::quota_status_view_model::CredentialStoreAvailability,
    quota::{
        current_unix_seconds, load_quota_status_report_with_availability_async,
        quota_status_view_model,
    },
    quota_display_model::QuotaStatusViewModel,
};
use codex_router_state::{
    account::{AccountRecord, AccountStatus},
    quota_snapshot::{PersistedQuotaSnapshot, QuotaSnapshotSource},
    sqlite::AsyncSqliteStateStore,
};
use serde::Serialize;
use std::{io, path::Path, sync::Arc};
type FixtureResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub(super) struct OwnedQuotaState {
    root: crate::quota::quota_account_adapter::fixture_factory::OwnedQuotaFixtureRoot,
    pub target: AccountActionTarget,
}
impl OwnedQuotaState {
    pub(super) async fn create(scenario: Scenario) -> FixtureResult<Self> {
        let root =
            crate::quota::quota_account_adapter::fixture_factory::OwnedQuotaFixtureRoot::create()?;
        let store = AsyncSqliteStateStore::open(&root.path().join("state.sqlite")).await?;
        let now = current_unix_seconds();
        let position = scenario.captured_position();
        let mut target = None;
        for index in 0..scenario.account_count() {
            let id = AccountId::new(format!("effect-account-{index}"))?;
            let label = format!("effect-{}", ["alpha", "beta", "gamma"][index]);
            let provider =
                if (scenario.is_login() || scenario == Scenario::GroupingReopen) && index == 1 {
                    Provider::Claude
                } else {
                    Provider::Openai
                };
            let status = if scenario.is_removal() && index == position {
                AccountStatus::Disabled
            } else {
                AccountStatus::Enabled
            };
            let record = AccountRecord::new(provider, id.clone(), &label, status)
                .with_active_credential_generation(7);
            store.upsert_account(&record).await?;
            store
                .upsert_quota_snapshot(
                    &PersistedQuotaSnapshot::new(id.clone(), QuotaSnapshotSource::MockEndpoint)
                        .with_observed_unix_seconds(now)
                        .with_route_band("responses", 50)
                        .with_reset_unix_seconds(now + 3600)
                        .with_reset_credits_available(2)
                        .with_stale_penalty(false),
                )
                .await?;
            if provider == Provider::Openai {
                write_credential(root.path(), &id, 7)?;
            }
            if scenario.is_login()
                && index == 0
                && !store.mark_generation_reauth_required(&id, 7).await?
            {
                return Err(
                    io::Error::other("fixture rejection generation was not current").into(),
                );
            }
            if index == position {
                target = Some(AccountActionTarget {
                    account_id: id,
                    label: QuotaAccountLabel::new(label)?,
                    provider,
                    position,
                });
            }
        }
        store.close().await?;
        Ok(Self {
            root,
            target: target.ok_or_else(|| io::Error::other("fixture has no captured account"))?,
        })
    }
    pub(super) fn root(&self) -> &Path {
        self.root.path()
    }
    pub(super) async fn with_account_commands<T>(
        &self,
        commands: agent_router_quota_tui::QuotaAccountCommands,
        future: impl std::future::Future<Output = T>,
    ) -> io::Result<T> {
        self.root.with_commands(commands, future).await
    }
    pub(super) async fn model(&self, width: usize) -> FixtureResult<QuotaStatusViewModel> {
        load_model(self.root(), width).await
    }
    pub(super) fn loader(&self, width: usize) -> QuotaStatusViewModelLoader {
        let root = self.root().to_owned();
        Arc::new(move || {
            let root = root.clone();
            Box::pin(async move { load_model(&root, width).await.ok() })
        })
    }
}

pub(super) async fn load_model(root: &Path, width: usize) -> FixtureResult<QuotaStatusViewModel> {
    let report = load_quota_status_report_with_availability_async(
        root,
        false,
        current_unix_seconds(),
        true,
        CredentialStoreAvailability::Ready,
    )
    .await?;
    Ok(quota_status_view_model(&report, report.rows(), width))
}

fn write_credential(root: &Path, id: &AccountId, generation: u64) -> FixtureResult<()> {
    use codex_router_secret_store::account_tokens::{
        AccountCredentialBundle, openai_account_credential_bundle_key,
    };
    use codex_router_secret_store::backend::SecretStore;
    let bundle = AccountCredentialBundle::imported_codex_auth(
        "synthetic-effect-access",
        Some("synthetic-effect-refresh".to_owned()),
    )
    .with_expires_unix_seconds(current_unix_seconds() + 86400)
    .with_chatgpt_account_id(format!("routing-{}", id.as_str()))
    .to_secret_string()?;
    let store = codex_router_secret_store::test_support::open_encrypted_credential_store(
        root.join("secrets"),
    )?;
    store.write_secret(
        &openai_account_credential_bundle_key(id, generation)?,
        &bundle,
    )?;
    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PersistedAccountFact {
    pub account_id: String,
    pub provider: String,
    pub label: String,
    pub status: String,
    pub credential_generation: Option<u64>,
}
pub(super) async fn read_accounts(root: &Path) -> FixtureResult<Vec<PersistedAccountFact>> {
    let store = AsyncSqliteStateStore::open_read_only(&root.join("state.sqlite")).await?;
    let records = store.list_accounts().await?;
    store.close().await?;
    Ok(records
        .into_iter()
        .map(|record| PersistedAccountFact {
            account_id: record.account_id().as_str().to_owned(),
            provider: record.provider().as_str().to_owned(),
            label: record.label().to_owned(),
            status: record.status().as_str().to_owned(),
            credential_generation: record.active_credential_generation(),
        })
        .collect())
}

pub(super) async fn change_status(
    root: &Path,
    id: &AccountId,
    status: AccountStatus,
) -> FixtureResult<()> {
    let writer = codex_router_state::sqlite::AsyncWeeklyQuotaFloorMutationStore::open(
        &root.join("state.sqlite"),
    )
    .await?;
    let result = writer.set_account_status_by_account_id(id, status).await;
    writer.close().await;
    result?;
    Ok(())
}

pub(super) async fn replace_login_generation(root: &Path, id: &AccountId) -> FixtureResult<()> {
    use codex_router_state::credential_maintenance::ClaimPurpose;
    let store = AsyncSqliteStateStore::open(&root.join("state.sqlite")).await?;
    if !store
        .claim_credential_refresh(
            id,
            Provider::Openai,
            ClaimPurpose::Login,
            7,
            8,
            current_unix_seconds(),
        )
        .await?
    {
        return Err(io::Error::other("fixture login generation claim was refused").into());
    }
    write_credential(root, id, 8)?;
    if !store
        .activate_claimed_credential_generation(
            id,
            Provider::Openai,
            ClaimPurpose::Login,
            7,
            8,
            current_unix_seconds(),
        )
        .await?
    {
        return Err(io::Error::other("fixture login generation activation was refused").into());
    }
    store.close().await?;
    Ok(())
}

pub(super) async fn read_policy_facts(root: &Path) -> FixtureResult<serde_json::Value> {
    use serde_json::json;
    let store = AsyncSqliteStateStore::open_read_only(&root.join("state.sqlite")).await?;
    let floors = store.list_account_routing_policies().await?.into_iter().map(|policy|json!({
        "accountId":policy.account_id().as_str(),"basisPoints":policy.weekly_quota_floor_basis_points().basis_points()
    })).collect::<Vec<_>>();
    let mut credits = Vec::new();
    for account in store.list_accounts().await? {
        let policy = store
            .load_account_credit_usage_policy(account.account_id())
            .await?;
        credits.push(
            json!({"accountId":account.account_id().as_str(),"policy":format!("{policy:?}")}),
        );
    }
    store.close().await?;
    Ok(json!({"floors":floors,"credits":credits}))
}
