//! Refusal and publication failures preserve prior local enrollment.
use super::*;

#[tokio::test]
async fn typed_non_success_never_changes_existing_or_absent_enrollment() -> TestResult {
    for existing in [false, true] {
        let fixture = ProducerFixture::start().await?;
        fixture
            .run(async |fixture| {
                let router_root = &fixture.router_root;
                let central = &fixture.central;
                if existing {
                    let first = fleet_cli::run(fleet_cli::enrollment(
                        router_root,
                        central,
                        &future_expiry(),
                    )?)
                    .await?;
                    fleet_cli::require_success(&first)?;
                }
                let before = fleet_cli::snapshot(router_root)?;
                let decisions = [
                    EnrollmentDecision::Refused(EnrollmentRefusal::PeerInUse),
                    EnrollmentDecision::Refused(EnrollmentRefusal::PeerMismatch),
                    EnrollmentDecision::Refused(EnrollmentRefusal::AccessClientInUse),
                    EnrollmentDecision::Refused(EnrollmentRefusal::CredentialHashInUse),
                    EnrollmentDecision::Refused(EnrollmentRefusal::InvalidInput),
                    EnrollmentDecision::RevocationInProgress,
                    EnrollmentDecision::EnrollmentInProgress,
                ];
                for decision in decisions {
                    central
                        .next_enrollment_reply(decision.clone())
                        .map_err(io::Error::other)?;
                    let calls = central.enrollment_calls();
                    let output = fleet_cli::run(fleet_cli::enrollment(
                        router_root,
                        central,
                        &future_expiry(),
                    )?)
                    .await?;
                    let reply = fleet_cli::require_success(&output)?;
                    assert_eq!(reply["decision"], serde_json::to_value(decision)?);
                    assert_eq!(
                        central.enrollment_calls(),
                        calls + 1,
                        "CLI retried an effectful request"
                    );
                    assert_eq!(before, fleet_cli::snapshot(router_root)?);
                    fleet_cli::assert_private(
                        &output,
                        &[central.owner_token(), central.access_client_secret()],
                    );
                }
                Ok::<_, Box<dyn Error>>(())
            })
            .await?;
    }
    Ok(())
}

#[tokio::test]
async fn invalid_expiry_or_owner_environment_fails_before_http_or_storage() -> TestResult {
    let fixture = ProducerFixture::start().await?;
    fixture
        .run(async |fixture| {
            let router_root = &fixture.router_root;
            let central = &fixture.central;
            for expiry in [
                "not-rfc3339".to_owned(),
                (Utc::now() - ChronoDuration::seconds(60)).to_rfc3339(),
            ] {
                let output =
                    fleet_cli::run(fleet_cli::enrollment(router_root, central, &expiry)?).await?;
                assert!(!output.status.success());
                assert!(String::from_utf8_lossy(&output.stderr).contains("--expires-at"));
                assert_eq!(central.enrollment_calls(), 0);
                assert!(fleet_cli::snapshot(router_root)?.is_empty());
            }
            for token in [None, Some(""), Some("invalid\r\nowner-token")] {
                let mut command = fleet_cli::enrollment(router_root, central, &future_expiry())?;
                if let Some(token) = token {
                    command.env(fleet_cli::OWNER_ENV, token);
                } else {
                    command.env_remove(fleet_cli::OWNER_ENV);
                }
                let output = fleet_cli::run(command).await?;
                assert!(!output.status.success());
                assert_eq!(central.enrollment_calls(), 0);
                assert!(fleet_cli::snapshot(router_root)?.is_empty());
                fleet_cli::assert_private(
                    &output,
                    &["invalid\r\nowner-token", central.access_client_secret()],
                );
            }
            assert!(!codex_router_host::router_collaboration_directory(router_root).exists());
            Ok::<_, Box<dyn Error>>(())
        })
        .await
}

#[tokio::test]
async fn malformed_redirect_and_mismatched_success_keep_old_state() -> TestResult {
    let fixture = ProducerFixture::start().await?;
    fixture
        .run(async |fixture| {
            let router_root = &fixture.router_root;
            let central = &fixture.central;
            let first = fleet_cli::run(fleet_cli::enrollment(
                router_root,
                central,
                &future_expiry(),
            )?)
            .await?;
            fleet_cli::require_success(&first)?;
            let before = fleet_cli::snapshot(router_root)?;
            for (status, body) in [
                (200, b"{broken-private-response-canary".to_vec()),
                (
                    200,
                    serde_json::to_vec(&serde_json::json!({"kind":"newUnexpectedDecision"}))?,
                ),
                (200, vec![b'x'; 32769]),
                (503, b"private-upstream-error-canary".to_vec()),
            ] {
                central
                    .next_raw_enrollment_reply(status, body)
                    .map_err(io::Error::other)?;
                let calls = central.enrollment_calls();
                let output = fleet_cli::run(fleet_cli::enrollment(
                    router_root,
                    central,
                    &future_expiry(),
                )?)
                .await?;
                assert!(!output.status.success());
                assert_eq!(central.enrollment_calls(), calls + 1);
                assert_eq!(before, fleet_cli::snapshot(router_root)?);
                fleet_cli::assert_private(
                    &output,
                    &[
                        "private-response-canary",
                        "private-upstream-error-canary",
                        central.owner_token(),
                        central.access_client_secret(),
                    ],
                );
            }
            central
                .next_enrollment_redirect(
                    central
                        .endpoint()
                        .replace("/v1/session", "/admin/v1/machines/enroll"),
                )
                .map_err(io::Error::other)?;
            let calls = central.enrollment_calls();
            let output = fleet_cli::run(fleet_cli::enrollment(
                router_root,
                central,
                &future_expiry(),
            )?)
            .await?;
            assert!(!output.status.success());
            assert!(String::from_utf8_lossy(&output.stderr).contains("307"));
            assert_eq!(
                central.enrollment_calls(),
                calls + 1,
                "CLI followed an owner-token redirect"
            );
            central
                .next_enrollment_reply(EnrollmentDecision::MachineEnrolled {
                    peer: "019f0000-0000-7000-8000-000000005999".parse()?,
                    expires_at: Utc::now() + ChronoDuration::hours(1),
                    authoritative_receiver: "019f0000-0000-7000-8000-000000000010".parse()?,
                    relay_peer: "019f0000-0000-7000-8000-000000005240".parse()?,
                })
                .map_err(io::Error::other)?;
            let output = fleet_cli::run(fleet_cli::enrollment(
                router_root,
                central,
                &future_expiry(),
            )?)
            .await?;
            assert!(!output.status.success());
            assert!(String::from_utf8_lossy(&output.stderr).contains("outcome is unknown"));
            assert_eq!(before, fleet_cli::snapshot(router_root)?);
            Ok::<_, Box<dyn Error>>(())
        })
        .await
}

#[tokio::test]
async fn publication_failure_removes_only_staged_generation_and_keeps_old_secrets() -> TestResult {
    use std::os::unix::fs::PermissionsExt;
    let fixture = ProducerFixture::start().await?;
    fixture
        .run(async |fixture| {
            let router_root = &fixture.router_root;
            let central = &fixture.central;
            let first = fleet_cli::run(fleet_cli::enrollment(
                router_root,
                central,
                &future_expiry(),
            )?)
            .await?;
            fleet_cli::require_success(&first)?;
            let before = fleet_cli::snapshot(router_root)?;
            let fleet = router_root.join("fleet");
            std::fs::set_permissions(&fleet, std::fs::Permissions::from_mode(0o500))?;
            let failed = fleet_cli::run(fleet_cli::enrollment(
                router_root,
                central,
                &future_expiry(),
            )?)
            .await;
            std::fs::set_permissions(&fleet, std::fs::Permissions::from_mode(0o700))?;
            let output = failed?;
            assert!(!output.status.success());
            assert!(String::from_utf8_lossy(&output.stderr).contains("metadata was not published"));
            assert_eq!(before, fleet_cli::snapshot(router_root)?);
            let status = fleet_cli::require_success(&fleet_cli::status(router_root).await?)?;
            assert_eq!(status["status"]["kind"], "valid");
            Ok::<_, Box<dyn Error>>(())
        })
        .await
}
