use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use codex_router_core::credit_usage::CreditAvailability;
use codex_router_core::credit_usage::CreditBalance;
use codex_router_core::credit_usage::CreditProviderLimitReason;
use codex_router_core::credit_usage::CreditProviderObservation;
use codex_router_core::credit_usage::CreditSpendControl;
use codex_router_core::ids::AccountId;
use codex_router_secret_store::account_tokens::AccountCredentialBundle;
use codex_router_secret_store::account_tokens::openai_account_credential_bundle_key;
use codex_router_secret_store::backend::SecretStore;
use codex_router_state::account::AccountRecord;
use codex_router_state::account::AccountStatus;
use codex_router_state::quota_snapshot::PersistedQuotaHistoryObservation;
use codex_router_state::quota_snapshot::PersistedQuotaSnapshot;
use codex_router_state::quota_snapshot::PersistedSelectorQuotaWindow;
use codex_router_state::quota_snapshot::QuotaHistoryRefreshOutcome;
use codex_router_state::quota_snapshot::QuotaSnapshotSource;
use codex_router_state::quota_snapshot::SelectorQuotaWindowStatus;
use codex_router_state::sqlite::AsyncSqliteStateStore;
use sha2::Digest;
use sha2::Sha256;

pub(super) type TestResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub(super) fn ensure(condition: bool, message: &'static str) -> TestResult<()> {
    if condition {
        Ok(())
    } else {
        Err(std::io::Error::other(message).into())
    }
}

static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(super) const ACCESS_TOKEN_CANARY: &str = "pty-access-token-canary";
const REFRESH_TOKEN_CANARY: &str = "pty-refresh-token-canary";
const FIXTURE_ROOT_MARKER_NAME: &str = ".codex-router-quota-reset-test-fixture";
const FIXTURE_ROOT_MARKER_PREFIX: &str = "codex-router-quota-reset-test-fixture:v1:";
pub(super) const FORBIDDEN_TERMINAL_CANARIES: &[&str] = &[
    ACCESS_TOKEN_CANARY,
    REFRESH_TOKEN_CANARY,
    "authorization: bearer",
    "chatgpt-account-id: routing-pty-alpha",
    "chatgpt-account-id: routing-pty-beta",
    "pty-credit-earliest",
];

pub(super) struct QuotaResetFixture {
    root: PathBuf,
    capability: String,
    state_bytes_before: Vec<u8>,
    secret_manifest_before: BTreeMap<String, String>,
}

#[derive(Clone, Copy)]
pub(super) enum ProviderFilterFixtureScenario {
    MixedAccounts,
    ClaudeAccountsDisabled,
}

impl QuotaResetFixture {
    pub(super) async fn create_provider_filters(
        scenario: ProviderFilterFixtureScenario,
    ) -> TestResult<Self> {
        let mut fixture = Self::create().await?;
        let state = AsyncSqliteStateStore::open(&fixture.root.join("state.sqlite")).await?;
        let claude_enabled = matches!(scenario, ProviderFilterFixtureScenario::MixedAccounts);
        for (provider, account_name, label, status, generation) in [
            (
                codex_router_core::provider::Provider::Openai,
                "acct_pty_alpha",
                "OpenAI Alpha",
                AccountStatus::Enabled,
                Some(7),
            ),
            (
                codex_router_core::provider::Provider::Openai,
                "acct_pty_beta",
                "OpenAI Beta",
                AccountStatus::Disabled,
                Some(11),
            ),
            (
                codex_router_core::provider::Provider::Claude,
                "acct_filter_gamma",
                "Claude Gamma",
                if claude_enabled {
                    AccountStatus::Enabled
                } else {
                    AccountStatus::Disabled
                },
                None,
            ),
            (
                codex_router_core::provider::Provider::Claude,
                "acct_filter_delta",
                "Claude Delta",
                AccountStatus::Disabled,
                None,
            ),
        ] {
            let account_id = AccountId::new(account_name)?;
            let account = AccountRecord::new(provider, account_id, label, status);
            let account = if let Some(generation) = generation {
                account.with_active_credential_generation(generation)
            } else {
                account
            };
            state.upsert_account(&account).await?;
        }
        state.close().await?;
        // Arrange must complete before the byte-level no-mutation baseline.
        fixture.state_bytes_before = fs::read(fixture.root.join("state.sqlite"))?;
        fixture.assert_secrets_unchanged()?;
        Ok(fixture)
    }

    /// Extra visual scene: normal reset/provider fixtures remain unchanged.
    pub(super) async fn create_visual_hard_cases() -> TestResult<Self> {
        let mut fixture =
            Self::create_provider_filters(ProviderFilterFixtureScenario::MixedAccounts).await?;
        let state = AsyncSqliteStateStore::open(&fixture.root.join("state.sqlite")).await?;
        let now = current_unix_seconds();
        for (name, generation, weekly, short, stale) in [
            ("acct_pty_alpha", 7, 92, Some(83), false),
            ("acct_pty_beta", 11, 75, None, true),
        ] {
            let account_id = AccountId::new(name)?;
            let observed = if stale { now.saturating_sub(3601) } else { now };
            let status = if stale {
                SelectorQuotaWindowStatus::Stale
            } else {
                SelectorQuotaWindowStatus::Eligible
            };
            let mut weekly_window =
                PersistedSelectorQuotaWindow::new(account_id.clone(), "responses", 604800, status)
                    .with_remaining_headroom(weekly)
                    .with_effective(true)
                    .with_observed_unix_seconds(observed);
            if !stale {
                weekly_window = weekly_window.with_reset_unix_seconds(now.saturating_add(576000));
            }
            let mut windows = vec![weekly_window];
            if let Some(remaining) = short {
                windows.push(
                    PersistedSelectorQuotaWindow::new(
                        account_id.clone(),
                        "responses",
                        18000,
                        SelectorQuotaWindowStatus::Eligible,
                    )
                    .with_remaining_headroom(remaining)
                    .with_effective(true)
                    .with_observed_unix_seconds(observed)
                    .with_reset_unix_seconds(now.saturating_add(7800)),
                );
            }
            let history = windows
                .iter()
                .map(|window| {
                    let observation = PersistedQuotaHistoryObservation::new(
                        account_id.clone(),
                        account_id.as_str(),
                        "responses",
                        window.limit_window_seconds(),
                        observed,
                        window.remaining_headroom(),
                    )
                    .with_effective(window.effective())
                    .with_window_status(window.status())
                    .with_refresh_source(QuotaSnapshotSource::OpenAiEndpoint)
                    .with_refresh_outcome(QuotaHistoryRefreshOutcome::Success);
                    window
                        .reset_unix_seconds()
                        .map_or(observation.clone(), |reset| {
                            observation.with_reset_unix_seconds(reset)
                        })
                })
                .collect::<Vec<_>>();
            let observation = CreditProviderObservation::new(
                CreditAvailability::Available {
                    balance: Some(CreditBalance::new(if stale {
                        "125.50"
                    } else {
                        "62500.00"
                    })?),
                },
                CreditSpendControl::Clear,
                None,
            );
            let snapshot = PersistedQuotaSnapshot::new(
                account_id.clone(),
                QuotaSnapshotSource::OpenAiEndpoint,
            )
            .with_observed_unix_seconds(observed)
            .with_route_band("responses", weekly)
            .with_stale_penalty(stale);
            let attempt = state
                .begin_credit_refresh_attempt(&account_id, generation)
                .await?;
            ensure(
                state
                    .record_responses_refresh_success(
                        codex_router_state::credit_store::ResponsesRefreshSuccessCommit {
                            attempt: &attempt,
                            selector_windows: &windows,
                            observed_unix_seconds: observed,
                            stale_after_unix_seconds: if stale {
                                now.saturating_sub(1)
                            } else {
                                now.saturating_add(3600)
                            },
                            provider_observation: &observation,
                            history_observations: &history,
                            snapshot: &snapshot,
                        },
                    )
                    .await?,
                "visual hard-case fixture commit must own its attempt",
            )?;
        }
        state.close().await?;
        fixture.state_bytes_before = fs::read(fixture.root.join("state.sqlite"))?;
        fixture.assert_secrets_unchanged()?;
        Ok(fixture)
    }

    pub(super) async fn create() -> TestResult<Self> {
        let sequence = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "codex-router-quota-reset-pty-{}-{}",
            std::process::id(),
            sequence
        ));
        fs::create_dir(&root)?;
        let capability = format!("pty-fixture-{}-{sequence}", std::process::id());
        fs::write(
            root.join(FIXTURE_ROOT_MARKER_NAME),
            format!("{FIXTURE_ROOT_MARKER_PREFIX}{capability}\n"),
        )?;
        let state_path = root.join("state.sqlite");
        let state = AsyncSqliteStateStore::open(&state_path).await?;
        let now = current_unix_seconds();
        for (account_id, label, generation, routing_id) in [
            ("acct_pty_alpha", "pty-alpha", 7, "routing-pty-alpha"),
            ("acct_pty_beta", "pty-beta", 11, "routing-pty-beta"),
        ] {
            let account_id = AccountId::new(account_id)?;
            state
                .upsert_account(
                    &AccountRecord::new(
                        codex_router_core::provider::Provider::Openai,
                        account_id.clone(),
                        label,
                        AccountStatus::Enabled,
                    )
                    .with_active_credential_generation(generation),
                )
                .await?;
            state
                .upsert_quota_snapshot(
                    &PersistedQuotaSnapshot::new(
                        account_id.clone(),
                        QuotaSnapshotSource::MockEndpoint,
                    )
                    .with_observed_unix_seconds(now)
                    .with_route_band("responses", 50)
                    .with_reset_unix_seconds(now.saturating_add(3_600))
                    .with_reset_credits_available(2)
                    .with_stale_penalty(false),
                )
                .await?;
            write_fixture_credential(&root, &account_id, generation, routing_id)?;
        }
        state.close().await?;
        let state_bytes_before = fs::read(&state_path)?;
        let secret_manifest_before = recursive_manifest(&root.join("secrets"))?;
        Ok(Self {
            root,
            capability,
            state_bytes_before,
            secret_manifest_before,
        })
    }

    pub(super) fn root(&self) -> &Path {
        &self.root
    }

    pub(super) fn capability(&self) -> &str {
        &self.capability
    }

    pub(super) fn assert_read_only(&self) -> TestResult<()> {
        let state_bytes_after = fs::read(self.root.join("state.sqlite"))?;
        ensure(
            self.state_bytes_before == state_bytes_after,
            "reset workflow changed fixture state bytes",
        )?;
        self.assert_secrets_unchanged()
    }

    pub(super) fn assert_secrets_unchanged(&self) -> TestResult<()> {
        ensure(
            self.secret_manifest_before == recursive_manifest(&self.root.join("secrets"))?,
            "quota TUI changed fixture secret manifest",
        )
    }

    pub(super) async fn seed_cached_credit_observation(
        &self,
        account_id: &AccountId,
        credential_generation: u64,
    ) -> TestResult<()> {
        let state = AsyncSqliteStateStore::open(&self.root.join("state.sqlite")).await?;
        let observed_unix_seconds = current_unix_seconds().saturating_sub(30);
        let stale_after_unix_seconds = observed_unix_seconds.saturating_add(3_600);
        let attempt = state
            .begin_credit_refresh_attempt(account_id, credential_generation)
            .await?;
        let windows = [18_000_u64, 604_800_u64].map(|window_seconds| {
            PersistedSelectorQuotaWindow::new(
                account_id.clone(),
                "responses",
                window_seconds,
                SelectorQuotaWindowStatus::Eligible,
            )
            .with_remaining_headroom(80)
            .with_effective(true)
            .with_observed_unix_seconds(observed_unix_seconds)
            .with_reset_unix_seconds(observed_unix_seconds.saturating_add(window_seconds))
        });
        let history = windows
            .iter()
            .map(|window| {
                PersistedQuotaHistoryObservation::new(
                    account_id.clone(),
                    account_id.as_str(),
                    "responses",
                    window.limit_window_seconds(),
                    observed_unix_seconds,
                    window.remaining_headroom(),
                )
                .with_reset_unix_seconds(
                    observed_unix_seconds.saturating_add(window.limit_window_seconds()),
                )
                .with_effective(window.effective())
                .with_window_status(window.status())
                .with_refresh_source(QuotaSnapshotSource::OpenAiEndpoint)
                .with_refresh_outcome(QuotaHistoryRefreshOutcome::Success)
            })
            .collect::<Vec<_>>();
        let snapshot =
            PersistedQuotaSnapshot::new(account_id.clone(), QuotaSnapshotSource::OpenAiEndpoint)
                .with_observed_unix_seconds(observed_unix_seconds)
                .with_route_band("responses", 80)
                .with_reset_unix_seconds(observed_unix_seconds.saturating_add(604_800))
                .with_stale_penalty(false);
        let observation = CreditProviderObservation::new(
            CreditAvailability::Available {
                balance: Some(CreditBalance::new("42.00")?),
            },
            CreditSpendControl::Clear,
            Some(CreditProviderLimitReason::RateLimitReached),
        );
        ensure(
            state
                .record_responses_refresh_success(
                    codex_router_state::credit_store::ResponsesRefreshSuccessCommit {
                        attempt: &attempt,
                        selector_windows: &windows,
                        observed_unix_seconds,
                        stale_after_unix_seconds,
                        provider_observation: &observation,
                        history_observations: &history,
                        snapshot: &snapshot,
                    },
                )
                .await?,
            "cached credit fixture should commit under its allocated attempt",
        )?;
        state.close().await?;
        Ok(())
    }
}

impl Drop for QuotaResetFixture {
    fn drop(&mut self) {
        if self.root.exists() {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}

fn write_fixture_credential(
    root: &Path,
    account_id: &AccountId,
    generation: u64,
    routing_id: &str,
) -> TestResult<()> {
    let bundle = AccountCredentialBundle::imported_codex_auth(
        ACCESS_TOKEN_CANARY,
        Some(REFRESH_TOKEN_CANARY.to_owned()),
    )
    .with_expires_unix_seconds(current_unix_seconds().saturating_add(86_400))
    .with_chatgpt_account_id(routing_id)
    .to_secret_string()?;
    let store = codex_router_secret_store::test_support::open_encrypted_credential_store(
        root.join("secrets"),
    )?;
    let key = openai_account_credential_bundle_key(account_id, generation)?;
    store.write_secret(&key, &bundle)?;
    Ok(())
}

fn current_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

fn recursive_manifest(root: &Path) -> TestResult<BTreeMap<String, String>> {
    let mut manifest = BTreeMap::new();
    collect_manifest(root, root, &mut manifest)?;
    Ok(manifest)
}

fn collect_manifest(
    root: &Path,
    path: &Path,
    manifest: &mut BTreeMap<String, String>,
) -> TestResult<()> {
    let metadata = fs::symlink_metadata(path)?;
    let relative = path.strip_prefix(root)?;
    let relative = if relative.as_os_str().is_empty() {
        ".".to_owned()
    } else {
        relative.to_string_lossy().into_owned()
    };
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode()
    };
    #[cfg(not(unix))]
    let mode = 0;
    if metadata.is_file() {
        let bytes = fs::read(path)?;
        manifest.insert(
            relative,
            format!("file:{mode:o}:{}:{:x}", bytes.len(), Sha256::digest(bytes)),
        );
        return Ok(());
    }
    if !metadata.is_dir() {
        return Err(std::io::Error::other("fixture manifest refuses special files").into());
    }
    manifest.insert(relative, format!("dir:{mode:o}"));
    let mut children = fs::read_dir(path)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    children.sort();
    for child in children {
        collect_manifest(root, &child, manifest)?;
    }
    Ok(())
}

#[tokio::test]
async fn visual_hard_cases_survive_canonical_store_readback_without_changing_normal_fixture()
-> TestResult<()> {
    let fixture = QuotaResetFixture::create_visual_hard_cases().await?;
    let state = AsyncSqliteStateStore::open_read_only(&fixture.root.join("state.sqlite")).await?;
    let inputs = state
        .selector_inputs_for_route_band("responses", current_unix_seconds())
        .await?;
    let alpha = inputs
        .iter()
        .find(|row| row.account_id().as_str() == "acct_pty_alpha")
        .ok_or_else(|| std::io::Error::other("missing Alpha"))?;
    let beta = inputs
        .iter()
        .find(|row| row.account_id().as_str() == "acct_pty_beta")
        .ok_or_else(|| std::io::Error::other("missing Beta"))?;
    let alpha_windows = alpha.windows();
    let beta_windows = beta.windows();
    ensure(
        alpha_windows.len() == 2,
        "Alpha must have two quota windows",
    )?;
    ensure(
        alpha_windows.iter().any(|window| {
            window.limit_window_seconds() == 604800
                && window.remaining_headroom() == 92
                && window.reset_unix_seconds().is_some()
        }),
        "Alpha weekly fixture facts mismatch",
    )?;
    ensure(
        alpha_windows.iter().any(|window| {
            window.limit_window_seconds() == 18000 && window.remaining_headroom() == 83
        }),
        "Alpha 5h fixture facts mismatch",
    )?;
    ensure(
        beta_windows.len() == 1,
        "Beta must have only its weekly quota window",
    )?;
    ensure(
        beta_windows.iter().any(|window| {
            window.limit_window_seconds() == 604800
                && window.remaining_headroom() == 75
                && window.reset_unix_seconds().is_none()
        }),
        "Beta known amount and unknown reset mismatch",
    )?;
    let credit = state
        .load_account_credit_observation(beta.account_id())
        .await?
        .ok_or_else(|| std::io::Error::other("missing Beta credits"))?;
    ensure(
        credit.provider_observation().availability()
            == &CreditAvailability::Available {
                balance: Some(CreditBalance::new("125.50")?),
            },
        "Beta exact credit balance mismatch",
    )?;
    ensure(
        credit
            .stale_after_unix_seconds()
            .is_some_and(|time| time < current_unix_seconds()),
        "Beta credits must be stale",
    )?;
    state.close().await?;
    fixture.assert_read_only()?;
    Ok(())
}
