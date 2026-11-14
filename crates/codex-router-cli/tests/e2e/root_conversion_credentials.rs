//! One-off conversion retains encrypted accounts on the real proxy resolver path.
#![cfg(all(debug_assertions, feature = "keychain-test-support"))]

use std::path::Path;

use codex_router_auth::resolver::{AsyncRefreshLeaseRegistry, AsyncRouterCredentialResolver};
use codex_router_core::{ids::AccountId, provider::Provider, redaction::SecretString};
use codex_router_secret_store::{
    SecretStore,
    account_tokens::{AccountCredentialBundle, provider_credential_bundle_key},
    credential_bundle::CredentialBundle,
    test_support::open_encrypted_credential_store,
};
use codex_router_state::{
    account::{AccountRecord, AccountStatus},
    sqlite::AsyncSqliteStateStore,
};
use tokio::process::Command;

fn require_proof(condition: bool, message: &str) -> Result<(), Box<dyn std::error::Error>> {
    if condition {
        Ok(())
    } else {
        Err(message.to_owned().into())
    }
}

#[tokio::test]
async fn converted_root_resolves_encrypted_credentials_and_lists_accounts()
-> Result<(), Box<dyn std::error::Error>> {
    // Arrange: real encrypted files, an active generation and closed SQLite handles.
    // Only Keychain access is replaced by the existing deterministic fixture key.
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let scratch = repository.join("tmp");
    std::fs::create_dir_all(&scratch)?;
    let directory = tempfile::Builder::new()
        .prefix("root-conversion-credentials-")
        .tempdir_in(&scratch)?;
    let router_root = directory.path().join("router");
    std::fs::create_dir(&router_root)?;
    let secret_root = router_root.join("secrets");
    let secrets = open_encrypted_credential_store(&secret_root)?;
    let state = AsyncSqliteStateStore::open(&router_root.join("state.sqlite")).await?;
    let mut fixtures = Vec::new();
    let now_unix_seconds = 1_000;
    for provider in [Provider::Openai, Provider::Claude] {
        let account_id = AccountId::new(uuid::Uuid::now_v7().to_string())?;
        let access = SecretString::new(uuid::Uuid::now_v7().to_string());
        let refresh = SecretString::new(uuid::Uuid::now_v7().to_string());
        let label = format!("conversion {} fixture", provider.as_str());
        let bundle = match provider {
            Provider::Openai => CredentialBundle::OpenAi(
                AccountCredentialBundle::imported_codex_auth(
                    access.expose_secret(),
                    Some(refresh.expose_secret().to_owned()),
                )
                .with_expires_unix_seconds(now_unix_seconds + 3_600),
            ),
            Provider::Claude => {
                CredentialBundle::new_claude(access.clone(), refresh, now_unix_seconds + 3_600)?
            }
        };
        secrets.write_secret(
            &provider_credential_bundle_key(provider, &account_id, 3)?,
            &bundle.to_secret_string()?,
        )?;
        state
            .upsert_account(
                &AccountRecord::new(provider, account_id.clone(), &label, AccountStatus::Enabled)
                    .with_active_credential_generation(3),
            )
            .await?;
        fixtures.push((provider, account_id, access, label));
    }
    state.close().await?;
    drop(secrets);

    // Act: run the actual one-off entrypoint on this disposable stopped root.
    let conversion = Command::new("node")
        .arg(repository.join("scripts/one-off/2026-11-convert-router-root.ts"))
        .arg("--root")
        .arg(&router_root)
        .output()
        .await?;
    require_proof(conversion.status.success(), "conversion must succeed")?;

    // Assert: reopen from disk, then resolve exactly as the production proxy does.
    let secrets = open_encrypted_credential_store(&secret_root)?;
    let state = AsyncSqliteStateStore::open_read_only(
        &codex_router_state::provider_proxy_database_path(&router_root),
    )
    .await?;
    let resolver = AsyncRouterCredentialResolver::new_default_oauth_with_refresh_leases(
        state.clone(),
        secrets,
        Some(now_unix_seconds),
        AsyncRefreshLeaseRegistry::new(),
    );
    for (provider, account_id, access, _) in &fixtures {
        let credential = resolver
            .resolve_provider_credentials(account_id, *provider)
            .await?;
        // Deliberately avoid an equality macro that could print a secret on failure.
        require_proof(
            credential.access_token() == access,
            "credential bytes must survive",
        )?;
        require_proof(
            credential.credential_generation() == 3,
            "active credential generation must survive",
        )?;
    }
    drop(resolver);
    state.close().await?;

    let binary = Path::new(env!("CARGO_BIN_EXE_codex-router"));
    let signing = Command::new(repository.join("scripts/cargo_debug_signing_runner.sh"))
        .arg("--sign-only")
        .arg(binary)
        .output()
        .await?;
    require_proof(signing.status.success(), "compiled CLI must be signed")?;
    let listed = Command::new(binary)
        .args(["account", "list", "--router-root"])
        .arg(&router_root)
        .env("OTEL_SDK_DISABLED", "true")
        .env_remove("CODEX_ROUTER_USE_HOME_DEFAULT")
        .output()
        .await?;
    require_proof(
        listed.status.success(),
        "compiled account list must succeed",
    )?;
    let stdout = String::from_utf8(listed.stdout)?;
    for (_, _, _, label) in &fixtures {
        require_proof(
            stdout.contains(label),
            "compiled CLI must list each converted account",
        )?;
    }
    Ok(())
}
