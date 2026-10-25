//! The actual uploader publishes every copy before recording workflow completion.
#![allow(clippy::panic_in_result_fn)]
mod support;
#[path = "upload_support/mod.rs"]
mod upload_support;
use board_legacy_upgrade::UpgradePublisher;
use collaboration_central_conformance::{JwksFixtureServer, owner_assertion};
use collaboration_central_core::{
    AdminImportPhase, AdminProjectImportAnswer, AdminProjectImportRequest,
};
use reqwest::header::{HeaderMap, HeaderValue};
use std::time::Duration;
use support::*;
#[path = "upload_support/central_fixture.rs"]
mod central_fixture;
use central_fixture::{owner_client, start_server};

#[derive(Clone, Copy, Debug)]
enum InterruptedAt {
    Publication,
    PublicationReceipt,
    CompletionReceipt,
}

#[tokio::test]
async fn real_project_import_reports_imported_and_non_imported_conflicts_without_copy_writes()
-> TestResult {
    let prepared = prepare("legacy-complete.sqlite").await?;
    let header = upload_support::header(&prepared).await?;
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start()
        .await
        .map_err(|error| error as Box<dyn std::error::Error>)?;
    let server = start_server(root.path(), &jwks).await?;
    let http = owner_client()?;
    let observed: TestResult = async {
        let publisher = UpgradePublisher::new(reqwest::Url::parse(server.base_url())?, http.clone());
        publisher.publish(&prepared).await?;
        let before = upload_support::published_copies(root.path(), &prepared, None).await?;
        let mut different = header.clone();
        different.digest = fleet_sync_protocol::ImportDigest::new("b".repeat(64))?;
        let answer: AdminProjectImportAnswer = http.post(format!("{}/admin/v1/import/projects/{}", server.base_url(), header.database()))
            .json(&AdminProjectImportRequest::Begin { header: different.clone() }).send().await?.error_for_status()?.json().await?;
        assert_eq!(answer, AdminProjectImportAnswer::ImportConflict { existing: Some(header.digest.clone()), offered: different.digest });
        assert_eq!(upload_support::published_copies(root.path(), &prepared, None).await?, before);
        let database = "019f0000-0000-7000-8000-000000000701";
        let create = serde_json::json!({"attempt":uuid::Uuid::now_v7(),"copy":"019f0000-0000-7000-8000-000000000010",
            "request":{"kind":"create","value":{"database":database,"epoch":1,"holder":"019f0000-0000-7000-8000-000000000702","origin":{"kind":"empty"}}}});
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let answer: serde_json::Value = http.post(format!("{}/admin/v1/conformance/copy-call", server.base_url()))
                    .json(&create).send().await?.error_for_status()?.json().await?;
                if answer == serde_json::json!({"kind":"completed","value":{"kind":"created","value":{"kind":"copyCreated"}}}) { break Ok::<(),Box<dyn std::error::Error>>(()); }
                if answer.get("kind").and_then(serde_json::Value::as_str) != Some("pending") { return Err(format!("ordinary CopyCreate refused: {answer}").into()); }
                tokio::task::yield_now().await;
            }
        }).await??;
        let copy = format!("copy-{database}");
        let original = upload_support::read(root.path(), &copy, "SELECT record AS value FROM _fleet_sync_copy").await?;
        let mut ordinary = header.clone();
        ordinary.manifest.database = fleet_sync_protocol::DatabaseId::try_from(database.to_owned())?;
        let answer: AdminProjectImportAnswer = http.post(format!("{}/admin/v1/import/projects/{database}", server.base_url()))
            .json(&AdminProjectImportRequest::Begin { header: ordinary }).send().await?.error_for_status()?.json().await?;
        assert_eq!(answer, AdminProjectImportAnswer::ImportConflict { existing: None, offered: header.digest });
        assert_eq!(upload_support::read(root.path(), &copy, "SELECT record AS value FROM _fleet_sync_copy").await?, original);
        Ok(())
    }.await;
    prepared.close().await;
    let stopped = server.shutdown().await;
    let fixture_stopped = jwks.shutdown().await;
    stopped?;
    fixture_stopped.map_err(|error| error as Box<dyn std::error::Error>)?;
    observed
}

#[tokio::test]
async fn actual_cli_publishes_and_relabels_a_completed_import_without_changing_source_or_copy()
-> TestResult {
    let (source_directory, source) = fixture_copy("legacy-complete.sqlite")?;
    assert_cli_publication_and_replay(source_directory.path(), &source, 3).await?;
    Ok(())
}

#[tokio::test]
async fn actual_cli_publishes_a_zero_post_project_with_only_its_project_state() -> TestResult {
    let (source_directory, source) = fixture_copy("legacy-complete.sqlite")?;
    let connection = rusqlite::Connection::open(&source)?;
    connection.execute(
        "INSERT INTO board_topics(topic_id,board_id,name,description) VALUES(?,?,?,?)",
        [
            "019a0000-0000-7000-8000-000000000019",
            "019a0000-0000-7000-8000-00000000000b",
            "ProjectState only",
            "A project without posts or activity",
        ],
    )?;
    connection.close().map_err(|(_, error)| error)?;

    let root = assert_cli_publication_and_replay(source_directory.path(), &source, 4).await?;
    let copy = "copy-019a0000-0000-7000-8000-000000000019";
    assert_eq!(
        upload_support::read(
            root.path(),
            copy,
            "SELECT json_array((SELECT count(*) FROM posts),(SELECT count(*) FROM project_feed),(SELECT count(*) FROM project_state),(SELECT max(position) FROM project_feed)) AS value",
        )
        .await?,
        vec!["[0,1,1,1]"]
    );
    let summaries = upload_support::read(
        root.path(),
        copy,
        "SELECT record AS value FROM _copy_summary",
    )
    .await?;
    let summary: collaboration_central_core::CopyDirectorySummary =
        serde_json::from_str(summaries.first().ok_or("copy summary absent")?)?;
    assert_eq!(
        summary.summary,
        board_sync_protocol::BoardAnnotation::ProjectSummary {
            latest_record_at: message_board::RecordTime::LegacyUnknown,
        }
    );
    Ok(())
}

async fn assert_cli_publication_and_replay(
    source_directory: &std::path::Path,
    source: &std::path::Path,
    project_count: usize,
) -> TestResult<tempfile::TempDir> {
    use std::os::unix::fs::PermissionsExt;
    let source_before = std::fs::read(source)?;
    let identity = source_directory.join("identity");
    std::fs::create_dir(&identity)?;
    let identity_path = create_host_identity(&identity)?;
    let identity_bytes = std::fs::read(&identity_path)?;
    let assertion = source_directory.join("assertion");
    std::fs::write(
        &assertion,
        owner_assertion().map_err(|error| error as Box<dyn std::error::Error>)?,
    )?;
    std::fs::set_permissions(&assertion, std::fs::Permissions::from_mode(0o600))?;
    let report = source_directory.join("report.json");
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start()
        .await
        .map_err(|error| error as Box<dyn std::error::Error>)?;
    let server = start_server(root.path(), &jwks).await?;
    let observed: TestResult = async {
        let mut before = None;
        for (owner, machine, expected) in [
            ("first-importer", "original source", "imported"),
            ("second-importer", "second source", "alreadyImported"),
        ] {
            let runner = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../scripts/cargo_debug_signing_runner.sh");
            let output = tokio::time::timeout(
                Duration::from_secs(45),
                tokio::process::Command::new(runner)
                    .arg(env!("CARGO_BIN_EXE_codex-router-board-upgrade"))
                    .arg("--source")
                    .arg(source)
                    .arg("--router-directory")
                    .arg(&identity)
                    .arg("--report")
                    .arg(&report)
                    .args([
                        "--central-url",
                        server.base_url(),
                        "--owner-human-id",
                        owner,
                        "--machine-label",
                        machine,
                    ])
                    .env(
                        "CODEX_ROUTER_DEBUG_CENTRAL_ACCESS_ASSERTION_FILE",
                        &assertion,
                    )
                    .kill_on_drop(true)
                    .output(),
            )
            .await??;
            let value: serde_json::Value = serde_json::from_slice(&std::fs::read(&report)?)?;
            assert_eq!(
                output.status.code(),
                Some(0),
                "report reason: {:?}; stderr: {}",
                value.get("reason"),
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(
                value.get("kind").and_then(serde_json::Value::as_str),
                Some("uploaded")
            );
            let results = value
                .pointer("/publication/projects")
                .and_then(serde_json::Value::as_array)
                .ok_or("publication projects absent")?;
            assert_eq!(results.len(), project_count);
            for project in results {
                assert_eq!(
                    project
                        .pointer("/answer/kind")
                        .and_then(serde_json::Value::as_str),
                    Some(expected)
                );
            }
            let copy = upload_support::read(
                root.path(),
                "copy-019a0000-0000-7000-8000-000000000015",
                upload_support::COPY_SNAPSHOT,
            )
            .await?;
            if let Some(before) = &before {
                assert_eq!(&copy, before);
            } else {
                before = Some(copy);
            }
        }
        assert_eq!(std::fs::read(source)?, source_before);
        assert_eq!(std::fs::read(&identity_path)?, identity_bytes);
        // Real maintenance owns the existing Host lock namespace (882). It
        // creates that lock, while preserving the identity and opening no socket.
        let mut files = std::fs::read_dir(identity)?
            .map(|entry| entry.map(|entry| entry.file_name()))
            .collect::<Result<Vec<_>, _>>()?;
        files.sort();
        assert_eq!(
            files,
            [
                std::ffi::OsString::from("agent-communication"),
                std::ffi::OsString::from("host.lock"),
            ]
        );
        Ok(())
    }
    .await;
    let stopped = server.shutdown().await;
    let fixture_stopped = jwks.shutdown().await;
    stopped?;
    fixture_stopped.map_err(|error| error as Box<dyn std::error::Error>)?;
    observed?;
    Ok(root)
}

#[tokio::test]
async fn real_store_failures_resume_before_and_after_publication_across_server_reopen() -> TestResult
{
    for interruption in [
        InterruptedAt::Publication,
        InterruptedAt::PublicationReceipt,
        InterruptedAt::CompletionReceipt,
    ] {
        let (sources, source) = fixture_copy("legacy-complete.sqlite")?;
        let prepared = support::prepare_path(&source).await?;
        let changed = board_legacy_upgrade::prepare_sources(
            &[source],
            provenance("relabelled computer")?,
            message_board::HumanId::try_from("different-importer".to_owned())?,
        )
        .await?;
        let root = tempfile::tempdir()?;
        let jwks = JwksFixtureServer::start()
            .await
            .map_err(|error| error as Box<dyn std::error::Error>)?;
        let server = start_server(root.path(), &jwks).await?;
        let http = owner_client()?;
        let header = upload_support::header(&prepared).await?;
        let project = prepared
            .report
            .projects
            .first()
            .ok_or("first project missing")?
            .project
            .clone();
        let route = format!("/admin/v1/import/projects/{}", project.as_str());
        let copy = format!("copy-{}", project.as_str());
        let first = async {
            let begun: AdminProjectImportAnswer = http.post(format!("{}{route}", server.base_url()))
                .json(&AdminProjectImportRequest::Begin { header: header.clone() }).send().await?.error_for_status()?.json().await?;
            assert!(matches!(begun, AdminProjectImportAnswer::Ready { .. }));
            let (object, fault) = match interruption {
                InterruptedAt::Publication => (copy.as_str(), "CREATE TRIGGER interrupt_import BEFORE INSERT ON _copy_summary BEGIN SELECT RAISE(ABORT,'publication interrupted'); END"),
                InterruptedAt::PublicationReceipt => ("fleet", "CREATE TRIGGER interrupt_import BEFORE UPDATE OF phase ON admin_imports WHEN json_extract(NEW.phase,'$.kind')='published' BEGIN SELECT RAISE(ABORT,'publication receipt interrupted'); END"),
                InterruptedAt::CompletionReceipt => ("fleet", "CREATE TRIGGER interrupt_import BEFORE UPDATE OF phase ON admin_imports WHEN json_extract(NEW.phase,'$.kind')='complete' BEGIN SELECT RAISE(ABORT,'completion receipt interrupted'); END"),
            };
            upload_support::execute(root.path(), object, fault).await?;
            let publisher = UpgradePublisher::new(reqwest::Url::parse(server.base_url())?, http.clone());
            assert!(publisher.publish(&prepared).await.is_err(), "{interruption:?} reported success");
            let row: AdminProjectImportAnswer = http.post(format!("{}{route}", server.base_url()))
                .json(&AdminProjectImportRequest::Status { digest: header.digest.clone() }).send().await?.error_for_status()?.json().await?;
            let AdminProjectImportAnswer::Progress(row) = row else { return Err("workflow status missing".into()); };
            assert_eq!(row.header, header);
            match interruption {
                InterruptedAt::Publication => {
                    assert_eq!(row.phase, AdminImportPhase::Reserved);
                    assert_eq!(upload_support::read(root.path(), &copy, "SELECT CAST(count(*) AS TEXT) AS value FROM sqlite_schema WHERE type='table' AND name='posts'").await?, vec!["0"]);
                    assert_eq!(upload_support::read(root.path(), &copy, "SELECT CAST(count(*) AS TEXT) AS value FROM _copy_summary").await?, vec!["0"]);
                    let staged = upload_support::read(root.path(), &copy, "SELECT payload AS value FROM _fleet_sync_seed_pages ORDER BY page").await?;
                    assert!(!staged.is_empty());
                    let wrong = publisher.publish(&changed).await.err().ok_or("changed unpublished header accepted")?;
                    assert!(matches!(wrong, board_legacy_upgrade::UpgradeUploadError::ProjectIncomplete {
                        answer, .. } if matches!(*answer, AdminProjectImportAnswer::Refused(collaboration_central_core::AdminImportRefusal::ImportAttemptInProgress { .. }))));
                    assert_eq!(upload_support::read(root.path(), &copy, "SELECT payload AS value FROM _fleet_sync_seed_pages ORDER BY page").await?, staged);
                    Ok(None)
                }
                InterruptedAt::PublicationReceipt | InterruptedAt::CompletionReceipt => {
                    assert_eq!(row.phase, match interruption { InterruptedAt::CompletionReceipt => AdminImportPhase::Published { position: header.manifest.position }, _ => AdminImportPhase::Reserved });
                    Ok(Some(upload_support::read(root.path(), &copy, upload_support::COPY_SNAPSHOT).await?))
                }
            }
        }.await;
        server.shutdown().await?;
        let first = match first {
            Ok(value) => value,
            Err(error) => {
                prepared.close().await;
                changed.close().await;
                jwks.shutdown()
                    .await
                    .map_err(|error| error as Box<dyn std::error::Error>)?;
                return Err(error);
            }
        };
        let object = if matches!(interruption, InterruptedAt::Publication) {
            copy.as_str()
        } else {
            "fleet"
        };
        upload_support::execute(root.path(), object, "DROP TRIGGER interrupt_import").await?;
        let reopened = start_server(root.path(), &jwks).await?;
        let resumed: TestResult = async {
            let offered = if first.is_some() { &changed } else { &prepared };
            let publisher =
                UpgradePublisher::new(reqwest::Url::parse(reopened.base_url())?, http.clone());
            let answer = publisher.publish(offered).await?;
            let completed = answer.projects.first().ok_or("first result missing")?;
            if let Some(before) = &first {
                assert_eq!(completed.answer, AdminProjectImportAnswer::AlreadyImported);
                assert_eq!(
                    &upload_support::read(root.path(), &copy, upload_support::COPY_SNAPSHOT)
                        .await?,
                    before
                );
            } else {
                assert_eq!(
                    completed.answer,
                    AdminProjectImportAnswer::Imported {
                        head: header.manifest.position
                    }
                );
            }
            let retained = first.as_ref().map(|_| &header);
            let snapshots =
                upload_support::published_copies(root.path(), offered, retained).await?;
            let projection =
                upload_support::projection(root.path(), offered.projects.len()).await?;
            let replay = publisher.publish(offered).await?;
            assert!(
                replay
                    .projects
                    .iter()
                    .all(|entry| entry.answer == AdminProjectImportAnswer::AlreadyImported)
            );
            assert_eq!(
                upload_support::published_copies(root.path(), offered, retained).await?,
                snapshots
            );
            assert_eq!(
                upload_support::projection(root.path(), offered.projects.len()).await?,
                projection
            );
            Ok(())
        }
        .await;
        prepared.close().await;
        changed.close().await;
        let stopped = reopened.shutdown().await;
        let fixture_stopped = jwks.shutdown().await;
        drop(sources);
        stopped?;
        fixture_stopped.map_err(|error| error as Box<dyn std::error::Error>)?;
        resumed?;
    }
    Ok(())
}

#[tokio::test]
async fn uploader_publishes_real_snapshots_and_replays_without_republishing() -> TestResult {
    let prepared = prepare("legacy-complete.sqlite").await?;
    let first = prepared
        .report
        .projects
        .first()
        .ok_or("first project missing")?;
    let project = first.project.clone();
    let digest = first.digest.clone();
    let manifest = first.manifest.clone();
    let jwks = JwksFixtureServer::start()
        .await
        .map_err(|error| error as Box<dyn std::error::Error>)?;
    let directory = tempfile::tempdir()?;
    let server = start_server(directory.path(), &jwks).await?;
    let observed = async {
        let mut headers = HeaderMap::new();
        let mut assertion = HeaderValue::from_str(
            &owner_assertion().map_err(|error| error as Box<dyn std::error::Error>)?,
        )?;
        assertion.set_sensitive(true);
        headers.insert("Cf-Access-Jwt-Assertion", assertion);
        let client = reqwest::Client::builder()
            .no_proxy()
            .default_headers(headers)
            .timeout(Duration::from_secs(10))
            .build()?;
        let publisher =
            UpgradePublisher::new(reqwest::Url::parse(server.base_url())?, client.clone());
        let uploaded = publisher.publish(&prepared).await?;
        assert_eq!(uploaded.projects.len(), prepared.projects.len());
        for (actual, expected) in uploaded.projects.iter().zip(&prepared.report.projects) {
            assert_eq!(actual.project, expected.project);
            assert_eq!(
                actual.answer,
                AdminProjectImportAnswer::Imported {
                    head: expected.manifest.position
                }
            );
        }
        let native_before =
            upload_support::published_copies(directory.path(), &prepared, None).await?;
        let projection =
            upload_support::projection(directory.path(), prepared.projects.len()).await?;
        let replay = publisher.publish(&prepared).await?;
        assert_eq!(replay.projects.len(), prepared.projects.len());
        for actual in replay.projects {
            assert_eq!(actual.answer, AdminProjectImportAnswer::AlreadyImported);
        }
        assert_eq!(
            upload_support::published_copies(directory.path(), &prepared, None).await?,
            native_before
        );
        assert_eq!(
            upload_support::projection(directory.path(), prepared.projects.len()).await?,
            projection
        );
        let response = client
            .post(format!(
                "{}/admin/v1/import/projects/{}",
                server.base_url(),
                project.as_str()
            ))
            .json(&AdminProjectImportRequest::Status { digest })
            .send()
            .await?
            .error_for_status()?;
        let AdminProjectImportAnswer::Progress(record) = response.json().await? else {
            return Err("staged workflow was not readable".into());
        };
        assert_eq!(
            record.phase,
            AdminImportPhase::Complete {
                position: manifest.position
            }
        );
        assert_eq!(record.header.manifest, manifest);
        assert_eq!(record.header.declared_owner, prepared.report.owner);
        assert!(record.header.pages > 0);
        let board_sync_protocol::BoardAnnotation::ProjectSummary { latest_record_at } =
            record.header.summary;
        assert_eq!(latest_record_at, message_board::RecordTime::LegacyUnknown);
        Ok(())
    }
    .await;
    prepared.close().await;
    let stopped = server.shutdown().await;
    let fixture_stopped = jwks.shutdown().await;
    stopped?;
    fixture_stopped.map_err(|error| error as Box<dyn std::error::Error>)?;
    observed
}
