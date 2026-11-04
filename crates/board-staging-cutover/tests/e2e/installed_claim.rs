//! Ordinary public Claim installs e2; cold inspection reads that actual physical file.
use super::{
    cutover_host_fixture::CutoverHostFixture,
    host_entrypoints::HOST_BASE_PREFIX,
    host_process::OwnedHost,
    native_snapshot_fixture::native_fixture,
    offline_cli_fixture::{RunFixture, TestResult},
};
use board_central_test_support::TestCentral;
use board_staging_cutover::{
    NativeProjectSnapshot, NativeSourceError, PreparedProjectBundle, SelectedWriterFile,
};
use collaboration_client::{BoardClientError, CollaborationClient};
use message_board::{ActorOrigin, BoardFailureKind, ProtocolVersion};
use std::{os::unix::fs::PermissionsExt, time::Duration};

#[tokio::test]
async fn ordinary_host_claim_installs_e2_with_exact_cold_replicated_tables() -> TestResult {
    cold_claim_case(SourceContents::ProjectOnly).await
}

#[tokio::test]
async fn ordinary_host_claim_preserves_an_emitted_intent_before_any_public_write() -> TestResult {
    cold_claim_case(SourceContents::EmittedIntent).await
}

#[tokio::test]
async fn ordinary_host_cutover_canary_reaches_a_distinct_enrolled_host() -> TestResult {
    cold_claim_case(SourceContents::EmittedIntentWithRemoteReadback).await
}

#[tokio::test]
async fn compiled_claim_command_replays_the_sealed_request_and_installs_actual_e2() -> TestResult {
    cold_claim_case(SourceContents::ProjectOnlyViaHelperClaim).await
}

#[derive(Clone, Copy)]
enum SourceContents {
    ProjectOnly,
    EmittedIntent,
    EmittedIntentWithRemoteReadback,
    ProjectOnlyViaHelperClaim,
}

async fn cold_claim_case(contents: SourceContents) -> TestResult {
    // Arrange existing native source, real central and an explicit short private runtime area.
    let base = tempfile::Builder::new()
        .prefix(HOST_BASE_PREFIX)
        .tempdir_in("/tmp")?;
    std::fs::set_permissions(base.path(), std::fs::Permissions::from_mode(0o700))?;
    let root = std::fs::canonicalize(base.path())?;
    let fixture =
        CutoverHostFixture::new_in(&root).map_err(|e| std::io::Error::other(e.to_string()))?;
    let mut source = native_fixture(&fixture)
        .await
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    let pending = if matches!(
        contents,
        SourceContents::EmittedIntent | SourceContents::EmittedIntentWithRemoteReadback
    ) {
        Some(super::pending_intent_fixture::add_pending_intent(&fixture, &mut source).await?)
    } else {
        None
    };
    let origin = ActorOrigin {
        installation: String::from(fixture.service.clone()).try_into()?,
        machine: "compiled cutover source".to_owned().try_into()?,
    };
    let central_area = tempfile::tempdir()?;
    let central = TestCentral::start_with_peer(central_area.path(), &origin, fixture.peer).await?;
    let artifacts = tempfile::tempdir()?;
    let result = async {
        let run = RunFixture::new(&fixture, &source, central.base_url(), artifacts.path())?;
        let published = run.execute().await?;
        assert!(
            published.status.success(),
            "{}",
            String::from_utf8_lossy(&published.stderr)
        );
        let bundle = PreparedProjectBundle::open(&run.bundle)?;
        if matches!(contents, SourceContents::ProjectOnlyViaHelperClaim) {
            super::direct_central_enrollment::install(&central, &fixture)?;
        } else {
            std::fs::create_dir_all(fixture.root.join("fleet"))?;
            let enrollment = central.write_host_session(&fixture.root)?;
            std::fs::rename(enrollment, fixture.root.join("fleet/enrollment.json"))?;
        }
        let host =
            OwnedHost::start(&fixture.root, &bundle.manifest().header.declared_owner).await?;
        let claimed = async {
            host.await_ready().await?;
            if matches!(contents, SourceContents::ProjectOnlyViaHelperClaim) {
                run.renew_owner(central.base_url())?;
                let claimed = run.execute_claim().await?;
                super::recovery_rows::require(
                    claimed.status.success(),
                    &format!(
                        "compiled Claim refused: {} {}",
                        String::from_utf8_lossy(&claimed.stdout),
                        String::from_utf8_lossy(&claimed.stderr)
                    ),
                )?;
                let first: serde_json::Value = serde_json::from_slice(&claimed.stdout)?;
                let projects = first
                    .get("projects")
                    .and_then(serde_json::Value::as_array)
                    .ok_or("compiled Claim project report missing")?;
                let [project] = projects.as_slice() else {
                    return Err("one compiled Claim project report required".into());
                };
                super::recovery_rows::require(
                    first.get("kind").and_then(serde_json::Value::as_str)
                        == Some("projectsClaimed")
                        && project.get("kind").and_then(serde_json::Value::as_str)
                            == Some("claimed")
                        && first.get("runId")
                            == Some(&serde_json::to_value(bundle.manifest().run_id)?)
                        && project.get("project")
                            == Some(&serde_json::to_value(&source.selected.project)?)
                        && project.get("request") == Some(&serde_json::to_value(&run.claim)?)
                        && project.get("epoch").and_then(serde_json::Value::as_u64) == Some(2)
                        && project.get("start")
                            == Some(&serde_json::to_value(
                                bundle.manifest().header.manifest.position,
                            )?),
                    "compiled Claim did not preserve the sealed request and expected e2 seed",
                )?;
                run.renew_owner(central.base_url())?;
                let replayed = run.execute_claim().await?;
                super::recovery_rows::require(
                    replayed.status.success(),
                    "compiled Claim replay failed",
                )?;
                super::recovery_rows::require(
                    serde_json::from_slice::<serde_json::Value>(&replayed.stdout)? == first,
                    "compiled Claim replay changed request or authority",
                )?;
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
            }
            let directory = codex_router_host::router_collaboration_directory(&fixture.root);
            let client =
                CollaborationClient::connect(&directory, "cutover-cold-readback", "1").await?;
            let request = board_sync_protocol::ProjectClaimRequest {
                request_id: run.claim.clone(),
                protocol_version: ProtocolVersion::CURRENT,
                project_id: source.selected.project.clone(),
            };
            // Act through the real Host's public route and machine-session installer.
            let answer = tokio::time::timeout(Duration::from_secs(20), async {
                loop {
                    match client.project_claim(request.clone()).await {
                        Ok(board_sync_protocol::ProjectClaimReply::Pending { .. }) => {
                            tokio::task::yield_now().await;
                        }
                        Err(BoardClientError::Rejected(error))
                            if error.kind == BoardFailureKind::BoardUnavailable =>
                        {
                            tokio::task::yield_now().await;
                        }
                        other => return other,
                    }
                }
            })
            .await??;
            assert!(matches!(
                answer,
                board_sync_protocol::ProjectClaimReply::Claimed { epoch, start, .. }
                    if epoch.value() == 2 && start == bundle.manifest().header.manifest.position
            ));
            drop(client);
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        }
        .await;
        let stopped = host.stop().await;
        claimed?;
        stopped?;
        // Assert exact installed content only after the actual Host and native child exit.
        let mut offline = fixture
            .acquire()
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        let mut installed = NativeProjectSnapshot::open(
            &mut offline,
            SelectedWriterFile {
                project: source.selected.project.clone(),
                peer: fixture.peer,
                file_epoch: fleet_sync_protocol::Epoch::new(2)?,
            },
        )
        .await?;
        let checked = async {
            std::fs::write(
                fixture.root.join("cold-preflight.json"),
                serde_json::to_vec(installed.preflight())?,
            )?;
            assert_eq!(installed.preflight().selected.file_epoch.value(), 2);
            let recovered = if let Some(before) = &pending {
                Some(
                    super::intent_recovery_proof::verify_recovered_intent(
                        &bundle,
                        &mut installed,
                        before,
                        fixture.peer,
                    )
                    .await?,
                )
            } else {
                assert_eq!(installed.preflight().application_epoch.get(), 1);
                assert_eq!(installed.preflight().high_water, source.cursor);
                installed.verify_replicated_tables(&bundle.manifest().header.manifest)?;
                let mut altered = bundle.manifest().header.manifest.clone();
                altered
                    .tables
                    .first_mut()
                    .ok_or("source tables absent")?
                    .digest = fleet_sync_protocol::TableDigest::new("0".repeat(64))?;
                assert!(matches!(
                    installed.verify_replicated_tables(&altered),
                    Err(NativeSourceError::ReplicatedTablesMismatch)
                ));
                let page = installed
                    .next_page()
                    .await?
                    .ok_or("installed snapshot page missing")?;
                assert_eq!(page.contents.database, source.manifest.database);
                None
            };
            assert!(
                bundle
                    .manifest()
                    .source_fingerprint
                    .matches(&bundle.manifest().backup_directory)?
            );
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(recovered)
        }
        .await;
        let closed = installed.close().await;
        let recovered = checked?;
        closed?;
        drop(offline);
        if let Some(recovered) = recovered {
            let head = recovered.head;
            let before = pending.as_ref().ok_or("recovery source missing")?;
            super::canary_proof::append_and_read(&fixture, &bundle, before, recovered).await?;
            let mut offline = fixture
                .acquire()
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            let installed = NativeProjectSnapshot::open(
                &mut offline,
                SelectedWriterFile {
                    project: source.selected.project.clone(),
                    peer: fixture.peer,
                    file_epoch: fleet_sync_protocol::Epoch::new(2)?,
                },
            )
            .await?;
            let counts = async {
                for (table, expected) in [
                    ("mail_handoffs", 1),
                    ("notify_intents", 1),
                    ("request_decisions", 2),
                    ("work_trail_rows", 1),
                ] {
                    let count = installed
                        .preflight()
                        .manifest
                        .tables
                        .iter()
                        .find(|item| item.table.as_str() == table)
                        .ok_or("post-canary table missing")?
                        .row_count;
                    super::recovery_rows::require(
                        count == expected,
                        "restart or canary added duplicate effects",
                    )?;
                }
                super::recovery_rows::require(
                    installed.preflight().high_water.position.get() == head.position.get() + 2,
                    "canary replay appended duplicate history",
                )
            }
            .await;
            let closed = installed.close().await;
            counts?;
            closed?;
            drop(offline);
            if matches!(contents, SourceContents::EmittedIntentWithRemoteReadback) {
                super::cross_machine_canary::read_on_another_machine(
                    &root, &fixture, &central, &bundle, head,
                )
                .await?;
            }
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let stopped = central.shutdown().await;
    if let Err(error) = result {
        let retained = fixture._area.keep();
        let _parent = base.keep();
        let prepared = artifacts.keep();
        return Err(format!(
            "{error}; owned fixture retained at {}; prepared artifacts at {}",
            retained.display(),
            prepared.display()
        )
        .into());
    }
    stopped?;
    Ok(())
}
