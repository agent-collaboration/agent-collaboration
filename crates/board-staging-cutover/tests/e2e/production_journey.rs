//! Compiled production mode carries a fake home through the existing real cutover path.
use super::{
    host_process::OwnedHost,
    offline_cli_fixture::{RunFixture, TestResult},
    production_mode::{execute, preview, production_fixture},
};
use board_central_test_support::TestCentral;
use board_staging_cutover::PreparedProjectBundle;
use message_board::ActorOrigin;
use std::os::unix::fs::MetadataExt;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assert real publication, physical custody and public writes."
)]
async fn compiled_production_mode_publishes_backs_up_claims_verifies_and_writes_on_a_fake_home()
-> TestResult {
    // Arrange a production-shaped temporary root and the existing authenticated TestCentral.
    let (fixture, source) = production_fixture().await?;
    let source_directory = source.path.parent().ok_or("source parent missing")?;
    let inode = source_directory.metadata()?.ino();
    let origin = ActorOrigin {
        installation: String::from(fixture.service.clone()).try_into()?,
        machine: "compiled cutover source".to_owned().try_into()?,
    };
    let central_area = tempfile::tempdir()?;
    let central = TestCentral::start_with_peer(central_area.path(), &origin, fixture.peer).await?;
    let result = async {
        let area = tempfile::tempdir()?;
        let run = RunFixture::new(&fixture, &source, central.base_url(), area.path())?;
        let plan = preview(&run, &fixture, central.base_url()).await?;
        // Retain a sample in the successful test output; no credentials enter the plan.
        println!(
            "sample production dry-run: {}",
            serde_json::to_string(&plan)?
        );
        // Act through compiled offline preparation/publication, with a fresh explicit confirmation.
        let published = execute(&run, &fixture, "offline", central.base_url()).await?;
        assert_eq!(published["kind"], "offlineCompleted");
        let bundle = PreparedProjectBundle::open(&run.bundle)?;
        assert_eq!(bundle.manifest().source.manifest, source.manifest);
        assert_eq!(bundle.manifest().backup_directory.metadata()?.ino(), inode);
        assert!(
            bundle
                .manifest()
                .source_fingerprint
                .matches(&bundle.manifest().backup_directory)?
        );
        assert!(!source.path.exists());
        // A fresh dry run permits idempotent replay of the same immutable import.
        run.renew_owner(central.base_url())?;
        let replay = execute(&run, &fixture, "offline", central.base_url()).await?;
        assert_eq!(
            replay["projects"][0]["importId"],
            published["projects"][0]["importId"]
        );
        assert_eq!(
            replay["projects"][0]["archive"]["kind"],
            "previouslyArchived"
        );
        super::direct_central_enrollment::install(&central, &fixture)?;
        let host =
            OwnedHost::start(&fixture.root, &bundle.manifest().header.declared_owner).await?;
        let claim = async {
            host.await_ready().await?;
            run.renew_owner(central.base_url())?;
            let claimed = execute(&run, &fixture, "claim", central.base_url())
                .await
                .map_err(|error| {
                    std::io::Error::other(format!("initial compiled Claim: {error}"))
                })?;
            assert_eq!(claimed["kind"], "projectsClaimed");
            assert_eq!(claimed["projects"][0]["epoch"], 2);
            assert_eq!(claimed["projects"][0]["request"], run.claim.as_str());
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        }
        .await;
        let stopped = host.stop().await;
        claim?;
        stopped?;
        // Assert cold actual e2 replicated tables before any canary write.
        let verified = execute(&run, &fixture, "verify", central.base_url()).await?;
        assert_eq!(verified["kind"], "contentVerified");
        assert_eq!(verified["projects"][0]["kind"], "verified");
        assert_eq!(
            verified["projects"][0]["installed"]["manifest"]["tables"],
            serde_json::to_value(&source.manifest.tables)?
        );
        let host =
            OwnedHost::start(&fixture.root, &bundle.manifest().header.declared_owner).await?;
        let canary = async {
            host.await_ready().await?;
            // Restart reattaches the writer through the same frozen Claim as the existing proof.
            run.renew_owner(central.base_url())?;
            let claimed = execute(&run, &fixture, "claim", central.base_url())
                .await
                .map_err(|error| {
                    std::io::Error::other(format!("reopened compiled Claim: {error}"))
                })?;
            assert_eq!(claimed["projects"][0]["request"], run.claim.as_str());
            let client = collaboration_client::CollaborationClient::connect(
                &codex_router_host::router_collaboration_directory(&fixture.root),
                "production-canary-proof",
                "1",
            )
            .await?;
            let before = client
                .view_work(message_board::ProjectViewRequest {
                    project_id: source.selected.project.clone(),
                    include_removed: true,
                })
                .await?
                .as_of;
            let expected = message_board::FeedCursor {
                epoch: message_board::EpochNumber::try_from(2)?,
                position: before
                    .position
                    .get()
                    .checked_add(1)
                    .ok_or("cursor overflow")?
                    .try_into()?,
            };
            let written = execute(&run, &fixture, "canary", central.base_url()).await?;
            assert_eq!(written["kind"], "canariesVerified");
            assert_eq!(written["projects"][0]["request"], run.canary.as_str());
            assert_eq!(
                written["projects"][0]["cursor"],
                serde_json::to_value(expected)?
            );
            assert_eq!(written["projects"][0]["disposition"], "created");
            // Read the actual public row before replay so a missing CLI write cannot be hidden.
            let read = client
                .work_trail_show(message_board::WorkTrailShowRequest {
                    reader: message_board::ReadActor::Anonymous {},
                    project_id: source.selected.project.clone(),
                    selection: message_board::WorkTrailSelection::Forward { after: before },
                    limit: message_board::PageLimit::try_from(10)?,
                    subject: None,
                    tags: vec![],
                    include_redacted: true,
                })
                .await?;
            let message_board::WorkTrailReadResult::Forward(page) = read.value else {
                return Err("canary read did not retain its project cursor".into());
            };
            let [row] = page.rows.as_slice() else {
                return Err("canary row missing or duplicated".into());
            };
            assert_eq!(row.row_id.as_str(), run.canary.as_str());
            assert_eq!(
                row.text.as_str(),
                format!("C5b cutover canary {}", bundle.manifest().run_id)
            );
            assert_eq!(row.envelope.request.as_ref(), Some(&run.canary));
            assert_eq!(row.envelope.position.project_cursor(), Some(expected));
            let replay = execute(&run, &fixture, "canary", central.base_url()).await?;
            assert_eq!(
                replay["projects"][0]["cursor"],
                written["projects"][0]["cursor"]
            );
            assert_eq!(
                replay["projects"][0]["headAfter"],
                written["projects"][0]["headAfter"]
            );
            assert_eq!(replay["projects"][0]["disposition"], "replayed");
            drop(client);
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        }
        .await;
        let stopped = host.stop().await;
        canary?;
        stopped?;
        // Production mode keeps fail-closed verification: the public canary changed the source.
        let plan = preview(&run, &fixture, central.base_url()).await?;
        let refused = super::production_mode::command(&run, &fixture, "verify")
            .args([
                "--production",
                "--target",
                central.base_url(),
                "--confirm-production",
                plan["digest"].as_str().ok_or("digest missing")?,
            ])
            .output()
            .await?;
        assert!(!refused.status.success());
        let refused: serde_json::Value = serde_json::from_slice(&refused.stdout)?;
        assert_eq!(refused["kind"], "contentUnverified");
        assert_eq!(refused["projects"][0]["kind"], "changed");
        assert!(
            bundle
                .manifest()
                .source_fingerprint
                .matches(&bundle.manifest().backup_directory)?
        );
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let stopped = central.shutdown().await;
    result?;
    stopped?;
    Ok(())
}
