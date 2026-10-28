//! BC05 publishes the immutable legacy fixture through O's actual CLI adapter.
use super::{
    central_joint,
    central_pair::{SecondHost, central},
    fixture::{BoardFixture, at},
    legacy_publisher::{PublishLegacyFixtureRequest, publish_legacy_fixture_to_central},
};
use codex_router_test_support::FixtureResult;
use sha2::{Digest, Sha256};
use std::{os::unix::fs::PermissionsExt, path::Path};

pub async fn joint(fixture: &mut BoardFixture, second: &mut SecondHost) -> FixtureResult {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let template =
        repository.join("crates/board-legacy-upgrade/tests/fixtures/legacy-complete.sqlite");
    let source = fixture.root.path().join("synthetic-legacy.sqlite");
    let before = Sha256::digest(std::fs::read(&template)?);
    std::fs::copy(&template, &source)?;
    std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o444))?;
    let owner = at(&fixture.owner, "/humanId")?
        .as_str()
        .ok_or("fixture owner human ID absent")?
        .to_owned();
    let central_url = central(fixture)?.base_url().to_owned();
    let assertion = collaboration_central_conformance::owner_assertion()?;
    fixture.stop_host().await?;
    let published = publish_legacy_fixture_to_central(PublishLegacyFixtureRequest {
        cli_binary: &repository.join("target/debug/codex-router-board-upgrade"),
        signing_runner: &repository.join("scripts/cargo_debug_signing_runner.sh"),
        source_copy: &source,
        router_directory: fixture.root.path(),
        report_path: &fixture.root.path().join("import-report.json"),
        central_url: &central_url,
        access_assertion: &assertion,
        owner_human_id: &owner,
        machine_label: "BC05 synthetic existing Host",
    })
    .await?;
    assert_eq!(Sha256::digest(std::fs::read(&source)?), before);
    assert_eq!(Sha256::digest(std::fs::read(&template)?), before);
    eprintln!(
        "BC05 actual CLI publication: projects={:?}, reportKind={:?}, sourceSha256={before:x}",
        published.project_ids,
        published.report.get("kind"),
    );
    fixture.project = published.claimable_project.as_str().to_owned();
    fixture.reopen_host().await?;
    central_joint::after_publication(fixture, second, published.unheld_project.as_str()).await
}
