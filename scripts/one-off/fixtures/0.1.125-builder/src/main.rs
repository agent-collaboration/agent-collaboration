//! Writes synthetic account/quota/affinity state through the released 0.1.125 writer.
use codex_router_core::{ids::AccountId, provider::Provider};
use codex_router_state::{
    account::{AccountRecord, AccountStatus},
    quota_snapshot::{PersistedQuotaSnapshot, QuotaSnapshotSource},
    session_account_affinity::SessionAccountAffinity,
    sqlite::AsyncSqliteStateStore,
};
use std::{error::Error, path::PathBuf};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let root = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("fresh output root required")?,
    );
    if root.exists() {
        return Err("refuse to overwrite fixture root".into());
    }
    std::fs::create_dir_all(&root)?;
    let account_id = AccountId::new("019f0000-0000-7000-8000-00000000e501")?;
    let writer = AsyncSqliteStateStore::open(&root.join("state.sqlite")).await?;
    writer
        .upsert_account(
            &AccountRecord::new(
                Provider::Openai,
                account_id.clone(),
                "Conversion fixture",
                AccountStatus::Disabled,
            )
            .with_active_credential_generation(3),
        )
        .await?;
    writer
        .upsert_quota_snapshot(
            &PersistedQuotaSnapshot::new(account_id.clone(), QuotaSnapshotSource::OpenAiEndpoint)
                .with_observed_unix_seconds(1_794_000_000)
                .with_route_band("responses", 4_200)
                .with_reset_unix_seconds(1_794_604_800)
                .with_stale_penalty(false),
        )
        .await?;
    writer
        .upsert_session_account_affinity(&SessionAccountAffinity::new(
            Provider::Openai,
            "019f0000-0000-7000-8000-00000000e502",
            account_id,
            1_794_000_100,
        ))
        .await?;
    // Preserve a genuine log-only committed row as a killed old Host would leave it.
    std::process::exit(0);
}
