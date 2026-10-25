//! The actual uploader publishes every copy before recording workflow completion.
#![allow(clippy::panic_in_result_fn)]
mod support;
#[allow(
    dead_code,
    reason = "The shared actual-CLI helper is consumed by sibling import integration targets."
)]
#[path = "upload_support/mod.rs"]
mod upload_support;
use board_legacy_upgrade::{PreparedUpgrade, UpgradePublisher};
use collaboration_central_core::{
    AdminImportPhase, AdminProjectImportAnswer, AdminProjectImportRequest,
};
use std::{error::Error, fmt::Display, time::Duration};
use support::*;
#[path = "upload_support/central_fixture.rs"]
mod central_fixture;
use central_fixture::{RestartableCentral, owner_client};
use codex_router_test_support::scenario_finalization::run_scenario;
#[path = "upload_support/publication_faults.rs"]
mod publication_faults;

type SafeResult<Value = ()> = codex_router_test_support::FixtureResult<Value>;

struct UploadScenario {
    central: RestartableCentral,
    prepared: Vec<PreparedUpgrade>,
}

impl UploadScenario {
    async fn start(prepared: Vec<PreparedUpgrade>) -> SafeResult<Self> {
        let central = match RestartableCentral::start().await {
            Ok(central) => central,
            Err(error) => {
                for project in prepared {
                    project.close().await;
                }
                return Err(promote_error(error));
            }
        };
        Ok(Self { central, prepared })
    }

    async fn shutdown(self) -> SafeResult {
        for project in self.prepared {
            project.close().await;
        }
        promote_result(self.central.shutdown().await)
    }
}

fn promote_error(error: impl Display) -> Box<dyn Error + Send + Sync> {
    Box::new(std::io::Error::other(error.to_string()))
}

fn promote_result<Value>(result: TestResult<Value>) -> SafeResult<Value> {
    result.map_err(promote_error)
}

#[tokio::test]
async fn real_project_import_reports_imported_and_non_imported_conflicts_without_copy_writes()
-> SafeResult {
    let prepared = promote_result(prepare("legacy-complete.sqlite").await)?;
    let header = promote_result(upload_support::header(&prepared).await)?;
    let scenario = UploadScenario::start(vec![prepared]).await?;

    run_scenario(
        scenario,
        async |scenario| {
            let prepared = scenario.prepared.first().ok_or("prepared import missing")?;
            let http = promote_result(owner_client())?;
            let endpoint = scenario
                .central
                .base_url()
                .map_err(promote_error)?
                .to_owned();
            let publisher = UpgradePublisher::new(reqwest::Url::parse(&endpoint)?, http.clone());
            publisher.publish(prepared).await.map_err(promote_error)?;
            scenario.central.stop_owner().await.map_err(promote_error)?;
            let before = promote_result(
                upload_support::published_copies(scenario.central.root(), prepared, None).await,
            )?;

            assert_imported_digest_conflict(scenario, &http, &header, &before).await?;
            assert_nonimported_copy_conflict(scenario, &http, &header).await?;
            Ok(())
        },
        UploadScenario::shutdown,
        || Ok(()),
    )
    .await
}

async fn assert_imported_digest_conflict(
    scenario: &mut UploadScenario,
    http: &reqwest::Client,
    header: &collaboration_central_core::AdminImportHeader,
    before: &[Vec<String>],
) -> SafeResult {
    let endpoint = scenario
        .central
        .reopen_owner()
        .await
        .map_err(promote_error)?;
    let mut different = header.clone();
    different.digest = fleet_sync_protocol::ImportDigest::new("b".repeat(64))?;
    let answer: AdminProjectImportAnswer = http
        .post(format!(
            "{}/admin/v1/import/projects/{}",
            endpoint,
            header.database()
        ))
        .json(&AdminProjectImportRequest::Begin {
            header: different.clone(),
        })
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(
        answer,
        AdminProjectImportAnswer::ImportConflict {
            existing: Some(header.digest.clone()),
            offered: different.digest,
        }
    );
    scenario.central.stop_owner().await.map_err(promote_error)?;
    let prepared = scenario.prepared.first().ok_or("prepared import missing")?;
    assert_eq!(
        promote_result(
            upload_support::published_copies(scenario.central.root(), prepared, None,).await
        )?,
        before
    );
    Ok(())
}

async fn assert_nonimported_copy_conflict(
    scenario: &mut UploadScenario,
    http: &reqwest::Client,
    header: &collaboration_central_core::AdminImportHeader,
) -> SafeResult {
    let database = "019f0000-0000-7000-8000-000000000701";
    let endpoint = scenario
        .central
        .reopen_owner()
        .await
        .map_err(promote_error)?;
    create_ordinary_copy(http, &endpoint, database).await?;
    scenario.central.stop_owner().await.map_err(promote_error)?;
    let copy = format!("copy-{database}");
    let original = promote_result(
        upload_support::read(
            scenario.central.root(),
            &copy,
            "SELECT record AS value FROM _fleet_sync_copy",
        )
        .await,
    )?;

    let endpoint = scenario
        .central
        .reopen_owner()
        .await
        .map_err(promote_error)?;
    let mut ordinary = header.clone();
    ordinary.manifest.database = fleet_sync_protocol::DatabaseId::try_from(database.to_owned())?;
    let answer: AdminProjectImportAnswer = http
        .post(format!("{endpoint}/admin/v1/import/projects/{database}"))
        .json(&AdminProjectImportRequest::Begin { header: ordinary })
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(
        answer,
        AdminProjectImportAnswer::ImportConflict {
            existing: None,
            offered: header.digest.clone(),
        }
    );
    scenario.central.stop_owner().await.map_err(promote_error)?;
    assert_eq!(
        promote_result(
            upload_support::read(
                scenario.central.root(),
                &copy,
                "SELECT record AS value FROM _fleet_sync_copy",
            )
            .await
        )?,
        original
    );
    Ok(())
}

async fn create_ordinary_copy(
    http: &reqwest::Client,
    endpoint: &str,
    database: &str,
) -> SafeResult {
    let create = serde_json::json!({"attempt":uuid::Uuid::now_v7(),"copy":"019f0000-0000-7000-8000-000000000010",
        "request":{"kind":"create","value":{"database":database,"epoch":1,"holder":"019f0000-0000-7000-8000-000000000702","origin":{"kind":"empty"}}}});
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let answer: serde_json::Value = http
                .post(format!("{endpoint}/admin/v1/conformance/copy-call"))
                .json(&create)
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            if answer == serde_json::json!({"kind":"completed","value":{"kind":"created","value":{"kind":"copyCreated"}}}) {
                return Ok::<(), Box<dyn Error + Send + Sync>>(());
            }
            if answer.get("kind").and_then(serde_json::Value::as_str) != Some("pending") {
                return Err(promote_error(format!("ordinary CopyCreate refused: {answer}")));
            }
            tokio::task::yield_now().await;
        }
    })
    .await??;
    Ok(())
}

#[tokio::test]
async fn real_store_failures_resume_before_and_after_publication_across_server_reopen() -> SafeResult
{
    publication_faults::run_all().await
}

#[tokio::test]
async fn uploader_publishes_real_snapshots_and_replays_without_republishing() -> SafeResult {
    let prepared = promote_result(prepare("legacy-complete.sqlite").await)?;
    let first = prepared
        .report
        .projects
        .first()
        .ok_or("first project missing")?;
    let project = first.project.as_str().to_owned();
    let digest = first.digest.clone();
    let manifest = first.manifest.clone();
    let header = promote_result(upload_support::header(&prepared).await)?;
    let owner = prepared.report.owner.clone();
    let scenario = UploadScenario::start(vec![prepared]).await?;

    run_scenario(
        scenario,
        async |scenario| {
            let prepared = scenario.prepared.first().ok_or("prepared import missing")?;
            let client = promote_result(owner_client())?;
            let endpoint = scenario
                .central
                .base_url()
                .map_err(promote_error)?
                .to_owned();
            let publisher = publication_faults::publisher_for(&endpoint, &client)?;
            let uploaded = publisher.publish(prepared).await.map_err(promote_error)?;
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
            assert_eq!(
                publication_faults::project_phase(&client, &endpoint, &project, &digest).await?,
                AdminImportPhase::Complete {
                    position: manifest.position
                }
            );
            scenario.central.stop_owner().await.map_err(promote_error)?;
            let snapshots = promote_result(
                upload_support::published_copies(scenario.central.root(), prepared, None).await,
            )?;
            let projection = promote_result(
                upload_support::projection(scenario.central.root(), prepared.projects.len()).await,
            )?;

            let replay_url = scenario
                .central
                .reopen_owner()
                .await
                .map_err(promote_error)?;
            let replay_publisher = publication_faults::publisher_for(&replay_url, &client)?;
            let replay = replay_publisher
                .publish(prepared)
                .await
                .map_err(promote_error)?;
            assert!(
                replay
                    .projects
                    .iter()
                    .all(|row| row.answer == AdminProjectImportAnswer::AlreadyImported)
            );
            let response = client
                .post(format!("{replay_url}/admin/v1/import/projects/{project}"))
                .json(&AdminProjectImportRequest::Status { digest })
                .send()
                .await?
                .error_for_status()?;
            let AdminProjectImportAnswer::Progress(record) = response.json().await? else {
                return Err(promote_error("staged workflow was not readable"));
            };
            assert_eq!(
                record.phase,
                AdminImportPhase::Complete {
                    position: manifest.position
                }
            );
            assert_eq!(record.header.manifest, manifest);
            assert_eq!(record.header.declared_owner, owner);
            assert_eq!(record.header, header);
            assert!(record.header.pages > 0);
            let board_sync_protocol::BoardAnnotation::ProjectSummary { latest_record_at } =
                record.header.summary;
            assert_eq!(latest_record_at, message_board::RecordTime::LegacyUnknown);

            scenario.central.stop_owner().await.map_err(promote_error)?;
            assert_eq!(
                promote_result(
                    upload_support::published_copies(scenario.central.root(), prepared, None,)
                        .await
                )?,
                snapshots
            );
            assert_eq!(
                promote_result(
                    upload_support::projection(scenario.central.root(), prepared.projects.len(),)
                        .await
                )?,
                projection
            );
            Ok(())
        },
        UploadScenario::shutdown,
        || Ok(()),
    )
    .await
}
