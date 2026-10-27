//! Status and declared debug storage exercise the same nonsecret metadata.
use super::*;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn status_rejects_malformed_or_unresolvable_metadata_without_http_or_mutation() -> TestResult
{
    use std::os::unix::fs::PermissionsExt;
    let fixture = ProducerFixture::start().await?;
    fixture
        .run(async |fixture| {
            let root = &fixture.router_root;
            let central = &fixture.central;
            let enrolled =
                fleet_cli::run(fleet_cli::enrollment(root, central, &future_expiry())?).await?;
            fleet_cli::require_success(&enrolled)?;
            let path = root.join("fleet/enrollment.json");
            let original = std::fs::read(&path)?;
            std::fs::write(&path, b"{malformed-metadata-private-canary")?;
            let before = fleet_cli::snapshot(root)?;
            let output = fleet_cli::status(root).await?;
            assert_eq!(
                fleet_cli::require_success(&output)?["status"]["kind"],
                "invalid"
            );
            assert_eq!(before, fleet_cli::snapshot(root)?);
            fleet_cli::assert_private(
                &output,
                &[
                    "malformed-metadata-private-canary",
                    central.access_client_secret(),
                    central.owner_token(),
                ],
            );
            std::fs::write(&path, original)?;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))?;
            let output = fleet_cli::status(root).await?;
            assert_eq!(
                fleet_cli::require_success(&output)?["status"]["kind"],
                "invalid"
            );
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
            let document = codex_router_host::read_fleet_enrollment_document(&path, root)?;
            let store = codex_router_host::open_fleet_enrollment_store(root)?;
            let (_, credential) = document.secret_keys()?;
            store.delete_staged(&credential)?;
            let before = fleet_cli::snapshot(root)?;
            let output = fleet_cli::status(root).await?;
            assert_eq!(
                fleet_cli::require_success(&output)?["status"]["kind"],
                "invalid"
            );
            assert_eq!(before, fleet_cli::snapshot(root)?);
            assert_eq!(central.enrollment_calls(), 1);
            assert_eq!(central.session_connections(), 0);
            Ok::<_, Box<dyn Error>>(())
        })
        .await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn explicit_debug_plaintext_declaration_uses_the_same_metadata_contract() -> TestResult {
    let fixture = ProducerFixture::start().await?;
    fixture.run(async |fixture| {
        let root = &fixture.router_root;
        let central = &fixture.central;
        let mut command = fleet_cli::enrollment(root, central, &future_expiry())?;
        command.arg("--allow-plaintext-file-secrets");
        let output = fleet_cli::run(command).await?;
        fleet_cli::require_success(&output)?;
        let document = codex_router_host::read_fleet_enrollment_document(&root.join("fleet/enrollment.json"), root)?;
        let secret_root = root.join("secrets");
        let store = codex_router_secret_store::file_backend::FileSecretStore::open_declared_debug_plaintext(&secret_root)?.ok_or("missing explicit debug declaration")?;
        let (access, credential) = document.secret_keys()?;
        assert_eq!(store.read_secret(&access)?.expose_secret(), central.access_client_secret());
        assert!(store.read_secret(&credential)?.expose_secret().starts_with("Bearer "));
        assert!(codex_router_host::load_fleet_enrollment(&root.join("fleet/enrollment.json"), root).is_ok());
        assert_eq!(fleet_cli::require_success(&fleet_cli::status(root).await?)?["status"]["kind"], "valid");
        let metadata = std::fs::read_to_string(root.join("fleet/enrollment.json"))?;
        assert!(!metadata.contains(central.access_client_secret()));
        fleet_cli::assert_private(&output, &[central.owner_token(), central.access_client_secret()]);
        Ok::<_, Box<dyn Error>>(())
    }).await
}
