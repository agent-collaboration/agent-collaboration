use super::*;

#[derive(Clone, Copy, Debug)]
enum InterruptedAt {
    Publication,
    PublicationReceipt,
    CompletionReceipt,
}

pub(super) async fn run_all() -> SafeResult {
    for interruption in [
        InterruptedAt::Publication,
        InterruptedAt::PublicationReceipt,
        InterruptedAt::CompletionReceipt,
    ] {
        run_interrupted_store_case(interruption).await?;
    }
    Ok(())
}

async fn run_interrupted_store_case(interruption: InterruptedAt) -> SafeResult {
    let (_sources, source) = promote_result(fixture_copy("legacy-complete.sqlite"))?;
    let prepared = promote_result(support::prepare_path(&source).await)?;
    let changed = board_legacy_upgrade::prepare_sources(
        std::slice::from_ref(&source),
        promote_result(provenance("relabelled computer"))?,
        message_board::HumanId::try_from("different-importer".to_owned())?,
    )
    .await
    .map_err(promote_error)?;
    let header = promote_result(upload_support::header(&prepared).await)?;
    let project = prepared
        .report
        .projects
        .first()
        .ok_or("first project missing")?
        .project
        .as_str()
        .to_owned();
    let copy = format!("copy-{project}");
    let scenario = UploadScenario::start(vec![prepared, changed]).await?;

    run_scenario(
        scenario,
        async |scenario| {
            let http = promote_result(owner_client())?;
            reserve_project_for_fault(scenario, &http, &header, &project).await?;
            scenario.central.stop_owner().await.map_err(promote_error)?;
            install_store_fault(scenario.central.root(), &copy, interruption).await?;
            let endpoint = scenario
                .central
                .reopen_owner()
                .await
                .map_err(promote_error)?;
            let publisher = publisher_for(&endpoint, &http)?;
            let original = scenario.prepared.first().ok_or("original import missing")?;
            assert_failed_publish(&publisher, original, interruption).await?;
            assert_stopped_phase(&http, &endpoint, &project, &header, interruption).await?;
            if matches!(interruption, InterruptedAt::Publication) {
                let changed = scenario.prepared.get(1).ok_or("changed import missing")?;
                assert_changed_offer_is_blocked(&publisher, changed).await?;
            }

            scenario.central.stop_owner().await.map_err(promote_error)?;
            let before_retry =
                read_failure_boundary(scenario.central.root(), &copy, interruption).await?;
            let fault_object = match interruption {
                InterruptedAt::Publication => copy.as_str(),
                InterruptedAt::PublicationReceipt | InterruptedAt::CompletionReceipt => "fleet",
            };
            promote_result(
                upload_support::execute(
                    scenario.central.root(),
                    fault_object,
                    "DROP TRIGGER interrupt_import",
                )
                .await,
            )?;
            resume_and_replay(scenario, &http, &header, &project, before_retry).await
        },
        UploadScenario::shutdown,
        || Ok(()),
    )
    .await
}

async fn reserve_project_for_fault(
    scenario: &UploadScenario,
    http: &reqwest::Client,
    header: &collaboration_central_core::AdminImportHeader,
    project: &str,
) -> SafeResult {
    let endpoint = scenario.central.base_url().map_err(promote_error)?;
    let answer: AdminProjectImportAnswer = http
        .post(format!("{endpoint}/admin/v1/import/projects/{project}"))
        .json(&AdminProjectImportRequest::Begin {
            header: header.clone(),
        })
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert!(matches!(answer, AdminProjectImportAnswer::Ready { .. }));
    Ok(())
}

async fn install_store_fault(
    root: &std::path::Path,
    copy: &str,
    interruption: InterruptedAt,
) -> SafeResult {
    let (object, trigger) = match interruption {
        InterruptedAt::Publication => (
            copy,
            "CREATE TRIGGER interrupt_import BEFORE INSERT ON _copy_summary BEGIN SELECT RAISE(ABORT,'publication interrupted'); END",
        ),
        InterruptedAt::PublicationReceipt => (
            "fleet",
            "CREATE TRIGGER interrupt_import BEFORE UPDATE OF phase ON admin_imports WHEN json_extract(NEW.phase,'$.kind')='published' BEGIN SELECT RAISE(ABORT,'publication receipt interrupted'); END",
        ),
        InterruptedAt::CompletionReceipt => (
            "fleet",
            "CREATE TRIGGER interrupt_import BEFORE UPDATE OF phase ON admin_imports WHEN json_extract(NEW.phase,'$.kind')='complete' BEGIN SELECT RAISE(ABORT,'completion receipt interrupted'); END",
        ),
    };
    promote_result(upload_support::execute(root, object, trigger).await)
}

pub(super) fn publisher_for(
    endpoint: &str,
    http: &reqwest::Client,
) -> SafeResult<UpgradePublisher> {
    Ok(UpgradePublisher::new(
        reqwest::Url::parse(endpoint)?,
        http.clone(),
    ))
}

async fn assert_failed_publish(
    publisher: &UpgradePublisher,
    prepared: &PreparedUpgrade,
    interruption: InterruptedAt,
) -> SafeResult {
    assert!(
        publisher.publish(prepared).await.is_err(),
        "{interruption:?} reported success"
    );
    Ok(())
}

async fn assert_stopped_phase(
    http: &reqwest::Client,
    endpoint: &str,
    project: &str,
    header: &collaboration_central_core::AdminImportHeader,
    interruption: InterruptedAt,
) -> SafeResult {
    let response = http
        .post(format!("{endpoint}/admin/v1/import/projects/{project}"))
        .json(&AdminProjectImportRequest::Status {
            digest: header.digest.clone(),
        })
        .send()
        .await?
        .error_for_status()?;
    let answer: AdminProjectImportAnswer = response.json().await?;
    let AdminProjectImportAnswer::Progress(progress) = answer else {
        return Err(promote_error("workflow status missing"));
    };
    assert_eq!(progress.header, *header);
    assert_eq!(
        progress.phase,
        match interruption {
            InterruptedAt::Publication | InterruptedAt::PublicationReceipt => {
                AdminImportPhase::Reserved
            }
            InterruptedAt::CompletionReceipt => AdminImportPhase::Published {
                position: header.manifest.position,
            },
        }
    );
    Ok(())
}

async fn assert_changed_offer_is_blocked(
    publisher: &UpgradePublisher,
    changed: &PreparedUpgrade,
) -> SafeResult {
    let error = publisher
        .publish(changed)
        .await
        .err()
        .ok_or_else(|| promote_error("changed unfinished import was accepted"))?;
    assert!(matches!(
        error,
        board_legacy_upgrade::UpgradeUploadError::ProjectIncomplete { answer, .. }
            if matches!(*answer, AdminProjectImportAnswer::Refused(
                collaboration_central_core::AdminImportRefusal::ImportAttemptInProgress { .. }
            ))
    ));
    Ok(())
}

async fn read_failure_boundary(
    root: &std::path::Path,
    copy: &str,
    interruption: InterruptedAt,
) -> SafeResult<Option<Vec<String>>> {
    match interruption {
        InterruptedAt::Publication => {
            let posts = promote_result(upload_support::read(
                root,
                copy,
                "SELECT CAST(count(*) AS TEXT) AS value FROM sqlite_schema WHERE type='table' AND name='posts'",
            )
            .await)?;
            let summaries = promote_result(
                upload_support::read(
                    root,
                    copy,
                    "SELECT CAST(count(*) AS TEXT) AS value FROM _copy_summary",
                )
                .await,
            )?;
            let staged = promote_result(
                upload_support::read(
                    root,
                    copy,
                    "SELECT payload AS value FROM _fleet_sync_seed_pages ORDER BY page",
                )
                .await,
            )?;
            assert_eq!(posts, vec!["0"]);
            assert_eq!(summaries, vec!["0"]);
            assert!(!staged.is_empty());
            Ok(None)
        }
        InterruptedAt::PublicationReceipt | InterruptedAt::CompletionReceipt => {
            let snapshot = promote_result(
                upload_support::read(root, copy, upload_support::COPY_SNAPSHOT).await,
            )?;
            assert_eq!(snapshot.len(), 1);
            let snapshot_row = snapshot.first().ok_or("copy snapshot missing")?;
            let record: serde_json::Value = serde_json::from_str(snapshot_row)?;
            assert!(
                record
                    .get("posts")
                    .and_then(serde_json::Value::as_u64)
                    .is_some_and(|count| count > 0)
            );
            Ok(Some(snapshot))
        }
    }
}

async fn resume_and_replay(
    scenario: &mut UploadScenario,
    http: &reqwest::Client,
    header: &collaboration_central_core::AdminImportHeader,
    project: &str,
    before_retry: Option<Vec<String>>,
) -> SafeResult {
    let offered_index = if before_retry.is_some() { 1 } else { 0 };
    let offered = scenario
        .prepared
        .get(offered_index)
        .ok_or("replay import missing")?;
    let retained_header = before_retry.as_ref().map(|_| header);
    let endpoint = scenario
        .central
        .reopen_owner()
        .await
        .map_err(promote_error)?;
    let publisher = publisher_for(&endpoint, http)?;
    let resumed = publisher.publish(offered).await.map_err(promote_error)?;
    let first = resumed.projects.first().ok_or("resumed project missing")?;
    assert_eq!(
        first.answer,
        if before_retry.is_some() {
            AdminProjectImportAnswer::AlreadyImported
        } else {
            AdminProjectImportAnswer::Imported {
                head: header.manifest.position,
            }
        }
    );
    assert_eq!(
        project_phase(http, &endpoint, project, &header.digest).await?,
        AdminImportPhase::Complete {
            position: header.manifest.position,
        }
    );
    scenario.central.stop_owner().await.map_err(promote_error)?;

    let copies = promote_result(
        upload_support::published_copies(scenario.central.root(), offered, retained_header).await,
    )?;
    let projection = promote_result(
        upload_support::projection(scenario.central.root(), offered.projects.len()).await,
    )?;
    let first_copy = promote_result(
        upload_support::read(
            scenario.central.root(),
            &format!("copy-{project}"),
            upload_support::COPY_SNAPSHOT,
        )
        .await,
    )?;
    if let Some(before) = &before_retry {
        assert_eq!(&first_copy, before);
    }

    let replay_url = scenario
        .central
        .reopen_owner()
        .await
        .map_err(promote_error)?;
    let replay_publisher = publisher_for(&replay_url, http)?;
    let replay = replay_publisher
        .publish(offered)
        .await
        .map_err(promote_error)?;
    assert!(
        replay
            .projects
            .iter()
            .all(|entry| entry.answer == AdminProjectImportAnswer::AlreadyImported)
    );
    scenario.central.stop_owner().await.map_err(promote_error)?;
    assert_eq!(
        promote_result(
            upload_support::published_copies(scenario.central.root(), offered, retained_header,)
                .await
        )?,
        copies
    );
    assert_eq!(
        promote_result(
            upload_support::projection(scenario.central.root(), offered.projects.len()).await,
        )?,
        projection
    );
    Ok(())
}

pub(super) async fn project_phase(
    http: &reqwest::Client,
    endpoint: &str,
    project: &str,
    digest: &fleet_sync_protocol::ImportDigest,
) -> SafeResult<AdminImportPhase> {
    let response = http
        .post(format!("{endpoint}/admin/v1/import/projects/{project}"))
        .json(&AdminProjectImportRequest::Status {
            digest: digest.clone(),
        })
        .send()
        .await?
        .error_for_status()?;
    let answer: AdminProjectImportAnswer = response.json().await?;
    let AdminProjectImportAnswer::Progress(progress) = answer else {
        return Err(promote_error("workflow status missing"));
    };
    Ok(progress.phase)
}
