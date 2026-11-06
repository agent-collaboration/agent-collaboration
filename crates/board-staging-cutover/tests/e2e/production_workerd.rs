//! Same compiled publisher against the real local Worker/DO storage adapter.
use super::{
    offline_cli_fixture::{RunFixture, TestResult},
    production_mode::{execute, production_fixture},
};
use board_staging_cutover::PreparedProjectBundle;
use collaboration_central_conformance::{JwksFixtureServer, workerd_process};
use collaboration_central_core::{
    AdminImportPhase, AdminProjectImportAnswer, AdminProjectImportRequest,
};
use std::{
    os::unix::fs::MetadataExt,
    path::PathBuf,
    time::{Duration, Instant},
};

#[tokio::test]
#[ignore = "requires the CI conformance Worker build and explicit pinned Node/Miniflare paths"]
async fn compiled_production_publication_completes_on_local_workerd() -> TestResult {
    workerd_publication_case(FleetReaderSetup::Empty).await
}

#[tokio::test]
#[ignore = "requires the CI conformance Worker build and explicit pinned Node/Miniflare paths"]
async fn compiled_production_publication_with_reader_notice_debt_completes_on_local_workerd()
-> TestResult {
    workerd_publication_case(FleetReaderSetup::RecordedReader).await
}

enum FleetReaderSetup {
    Empty,
    RecordedReader,
}

async fn workerd_publication_case(readers: FleetReaderSetup) -> TestResult {
    // Arrange a real plain native source, fake production home and isolated Worker persistence.
    let node =
        PathBuf::from(std::env::var_os("D1_WORKERD_NODE").ok_or("D1_WORKERD_NODE required")?);
    let miniflare = PathBuf::from(
        std::env::var_os("D1_MINIFLARE_ENTRYPOINT").ok_or("D1_MINIFLARE_ENTRYPOINT required")?,
    );
    if !node.is_absolute() || !miniflare.is_absolute() {
        return Err("absolute pinned tool paths required".into());
    }
    let (fixture, source) = production_fixture().await?;
    let inode = source
        .path
        .parent()
        .ok_or("source parent missing")?
        .metadata()?
        .ino();
    let jwks = JwksFixtureServer::start().await?;
    let persistence = tempfile::tempdir()?;
    let port = std::net::TcpListener::bind("127.0.0.1:0")?
        .local_addr()?
        .port();
    let target = format!("http://127.0.0.1:{port}");
    let settings = workerd_process::WorkerdSettings {
        node,
        miniflare,
        persistence: persistence.path().to_owned(),
        port,
        jwks_url: jwks.url.clone(),
        holder_answer_timeout: Duration::from_secs(2),
        copy_ack_wait: Duration::from_secs(2),
        relay_peer: "019f0000-0000-7000-8000-000000005240"
            .to_owned()
            .try_into()?,
    };
    let worker = workerd_process::WorkerdProcess::start(&settings).await?;
    let result = async {
        if matches!(readers, FleetReaderSetup::RecordedReader) {
            // Existing public scenario records a real Fleet reader; the row survives socket close.
            let client = collaboration_central_conformance::ConformanceClient::new(&target)?;
            collaboration_central_conformance::run_feed_history_conformance(&client).await?;
            println!("WORKERD_PRODUCTION_READER_SETUP recorded Fleet reader before import");
        }
        let area = tempfile::tempdir()?;
        let run = RunFixture::new(&fixture, &source, &target, area.path())?;
        let started = Instant::now();
        // Act through the normal signed CLI and owner JWT verification, not TestCentral.
        let report = execute(&run, &fixture, "offline", &target).await?;
        println!("WORKERD_PRODUCTION_PUBLICATION elapsed_ms={} report={}", started.elapsed().as_millis(), serde_json::to_string(&report)?);
        assert_eq!(report.get("kind").and_then(serde_json::Value::as_str), Some("offlineCompleted"));
        let bundle = PreparedProjectBundle::open(&run.bundle)?;
        assert_eq!(bundle.manifest().source.manifest, source.manifest);
        assert_eq!(bundle.manifest().backup_directory.metadata()?.ino(), inode);
        assert!(bundle.manifest().source_fingerprint.matches(&bundle.manifest().backup_directory)?);
        assert!(!source.path.exists());
        // Assert the actual Worker workflow owns the exact completed import header and position.
        let status: AdminProjectImportAnswer = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(10)).build()?
            .post(format!("{target}/admin/v1/import/projects/{}", source.selected.project.as_str()))
            .header("Cf-Access-Jwt-Assertion", collaboration_central_conformance::owner_assertion()?)
            .json(&AdminProjectImportRequest::Status { digest: bundle.manifest().header.digest.clone() })
            .send().await?.error_for_status()?.json().await?;
        let AdminProjectImportAnswer::Progress(record) = status else { return Err("Worker import continuation missing".into()); };
        assert_eq!(record.header, bundle.manifest().header);
        assert_eq!(record.phase, AdminImportPhase::Complete { position: record.header.manifest.position });
        println!("WORKERD_PRODUCTION_PUBLICATION_RESULT 1 passed; 0 failed; normal compiled production helper, real Fleet/Copy objects, exact complete header and original backup");
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    let stopped = worker.stop().await;
    let closed = jwks.shutdown().await;
    result?;
    stopped?;
    closed?;
    Ok(())
}
