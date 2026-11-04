//! Signed compiled CLI journey on isolated roots, using only synthetic credentials.
#![cfg(all(debug_assertions, feature = "keychain-test-support"))]

use std::path::{Path, PathBuf};
use std::process::Output;

use codex_router_cli::account::{AccountImportRequest, import_codex_auth_from_request_async};
use codex_router_core::ids::AccountId;
use codex_router_secret_store::SecretStore;
use codex_router_secret_store::account_tokens::openai_account_credential_bundle_key;
use codex_router_secret_store::file_backend::FileSecretStore;
use codex_router_secret_store::runtime_credential_store::RuntimeCredentialStore;
use codex_router_secret_store::test_support::open_encrypted_credential_store;
use codex_router_state::sqlite::AsyncSqliteStateStore;
use sha2::{Digest, Sha256};
use tokio::process::Command;

type ProofResult<TValue> = Result<TValue, Box<dyn std::error::Error>>;

fn check(condition: bool, message: &str) -> ProofResult<()> {
    if condition {
        Ok(())
    } else {
        Err(message.to_owned().into())
    }
}

struct RemovalJourney {
    binary: PathBuf,
    router_root: PathBuf,
    app_server_socket: PathBuf,
}

impl RemovalJourney {
    async fn execute(&self, arguments: &[&str]) -> ProofResult<Output> {
        let output = Command::new(&self.binary)
            .args(arguments)
            .arg("--router-root")
            .arg(&self.router_root)
            .env(
                "CODEX_ROUTER_DEBUG_APP_SERVER_SOCKET",
                &self.app_server_socket,
            )
            .env("OTEL_SDK_DISABLED", "true")
            .env_remove("CODEX_ROUTER_USE_HOME_DEFAULT")
            .kill_on_drop(true)
            .output()
            .await?;
        println!(
            "{:?}: exit={:?}; stdout={}; stderr={}",
            arguments,
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(output)
    }
}

#[tokio::test]
async fn compiled_account_removal_journey_on_encrypted_and_plaintext_debug_roots() -> ProofResult<()>
{
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let binary = PathBuf::from(env!("CARGO_BIN_EXE_codex-router"));
    let signed = Command::new(repository.join("scripts/cargo_debug_signing_runner.sh"))
        .arg("--sign-only")
        .arg(&binary)
        .output()
        .await?;
    check(signed.status.success(), "debug signing runner must succeed")?;
    let hash = Sha256::digest(std::fs::read(&binary)?);
    println!("candidate={}; sha256={hash:x}", binary.display());

    for plaintext in [false, true] {
        let directory = tempfile::tempdir()?;
        let router_root = directory.path().join("router");
        let socket_root = directory.path().join("native-socket");
        std::fs::create_dir(&socket_root)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&socket_root, std::fs::Permissions::from_mode(0o700))?;
        }
        let journey = RemovalJourney {
            binary: binary.clone(),
            router_root: router_root.clone(),
            app_server_socket: socket_root.join("app-server.sock"),
        };
        let secret_root = router_root.join("secrets");
        let secrets = if plaintext {
            RuntimeCredentialStore::DebugPlaintext(FileSecretStore::initialize_debug_plaintext(
                &secret_root,
            )?)
        } else {
            open_encrypted_credential_store(&secret_root)?.into()
        };
        let account_id = AccountId::new("acct_removal_fixture")?;
        let state = AsyncSqliteStateStore::open(&router_root.join("state.sqlite")).await?;
        import_codex_auth_from_request_async(
            &state,
            &secrets,
            AccountImportRequest::new(account_id.clone(), "removal fixture", "synthetic-access")
                .with_refresh_token("synthetic-refresh"),
        )
        .await?;
        state.close().await?;
        let key = openai_account_credential_bundle_key(&account_id, 1)?;
        check(
            secrets.read_secret(&key).is_ok(),
            "fixture credential must exist before removal",
        )?;
        let listed = journey.execute(&["account", "list"]).await?;
        check(
            listed.status.success()
                && String::from_utf8_lossy(&listed.stdout).contains("removal fixture"),
            "list must show the enabled fixture",
        )?;
        let quota_before = journey
            .execute(&["quota", "--format", "json", "--now-unix-seconds", "1000"])
            .await?;
        check(
            quota_before.status.success()
                && String::from_utf8_lossy(&quota_before.stdout).contains("removal fixture"),
            "Quota must show the fixture before removal",
        )?;
        let refused = journey
            .execute(&["account", "remove", "--account", "removal fixture"])
            .await?;
        let refusal = String::from_utf8_lossy(&refused.stderr);
        check(
            !refused.status.success()
                && refusal.contains("removal fixture")
                && refusal.contains("disable"),
            "enabled removal must name the account and require disable first",
        )?;
        check(
            secrets.read_secret(&key).is_ok(),
            "refusal must preserve credentials",
        )?;
        check(
            journey
                .execute(&["account", "disable", "--account", "removal fixture"])
                .await?
                .status
                .success(),
            "disable must succeed",
        )?;
        let removed = journey
            .execute(&["account", "remove", "--account", "removal fixture"])
            .await?;
        check(
            removed.status.success() && removed.stdout == b"removed account: removal fixture\n",
            "disabled removal must confirm in one line",
        )?;
        let listed = journey.execute(&["account", "list"]).await?;
        check(
            listed.status.success()
                && !String::from_utf8_lossy(&listed.stdout).contains("removal fixture"),
            "removed account must disappear from account list",
        )?;
        let quota = journey
            .execute(&["quota", "--format", "json", "--now-unix-seconds", "1000"])
            .await?;
        check(
            quota.status.success()
                && !String::from_utf8_lossy(&quota.stdout).contains("removal fixture"),
            "removed account must disappear from Quota",
        )?;
        check(
            secrets.read_secret(&key).is_err(),
            "secret store must no longer contain the fixture credential",
        )?;
        let state =
            AsyncSqliteStateStore::open_read_only(&router_root.join("state.sqlite")).await?;
        check(
            state.load_account(&account_id).await?.is_none(),
            "owning state reopen must show the account absent",
        )?;
        state.close().await?;
        drop(secrets);
        directory.close()?;
        println!("plaintext={plaintext}: journey passed; owned debug root cleaned");
    }
    Ok(())
}
