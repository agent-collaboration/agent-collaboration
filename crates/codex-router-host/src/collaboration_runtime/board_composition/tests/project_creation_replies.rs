//! Directory acceptance survives a delayed real Host registration without a false error.
use super::*;
use codex_router_test_support::FixtureResult;
use collaboration_mcp::test_support::ServedCollaborationApi;
use collaboration_service::ApplicationFixture;
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;

async fn assert_creation_reply(ready_during_wait: bool) -> TestResult {
    let directory = tempfile::tempdir()?;
    let root = directory.path().join("fresh-router");
    let mut central =
        TestFleet::start(&directory.path().join("central"), GateAnswer::Admitted).await?;
    let board = open_board_admission(&root, identity()?, Some(central.inputs()?)).await?;
    let session = board.session.as_ref().ok_or("session")?;
    let ready = session.welcomed.subscribe();
    let projects = session.projects.clone();
    let admission = board.admission.clone();
    let (fleet, creation) = arrange_project_input(&board, &root).await?;
    let project_file = root
        .join("projects")
        .join(creation.project_id.as_str())
        .join("e1.sqlite");
    assert!(!project_file.exists());
    let api_root = directory.path().join("public-api");
    tokio::fs::create_dir_all(&api_root).await?;
    std::fs::set_permissions(&api_root, std::fs::Permissions::from_mode(0o700))?;
    let application = (ApplicationFixture::new(
        "550e8400-e29b-41d4-a716-446655440000",
        "00000000-0000-4000-8000-000000000002",
    )?
    .with_project_admission(admission.clone()))
    .build();
    let api = ServedCollaborationApi::start(&api_root, application)?;
    let input = serde_json::to_value(&creation)?;
    // The real session pump has not started, so only its existing request
    // deadline can answer PendingUnreachable. Its local IO uses paused Tokio.
    tokio::time::pause();
    let queued = api.call("project_create", input.clone()).await;
    tokio::time::resume();
    let queued = queued?;
    assert_eq!(
        queued.pointer("/result/result/kind"),
        Some(&json!("pendingUnreachable")),
        "{queued}"
    );
    let running = board.start().ok_or("running session")?;
    timeout(BOUND, central.auth.entered.notified()).await?;
    settle_fixture_directory(&fleet, &creation).await?;

    let created = if ready_during_wait {
        let mut waits = projects.subscribe_registration_waits();
        let (reply, released) = tokio::join!(
            timeout(BOUND, api.call("project_create", input.clone())),
            async {
                let project = timeout(BOUND, waits.recv()).await??;
                assert_eq!(project, creation.project_id);
                central.release()?;
                wait_welcome(ready.clone()).await
            }
        );
        released?;
        let reply = reply??;
        assert_eq!(
            reply.pointer("/result/result/kind"),
            Some(&json!("decided")),
            "Ready inside the wait did not complete creation: {reply}"
        );
        reply
    } else {
        // Keep the real Welcome held and drive only the existing caller deadline.
        let pending = expire_registration_wait(
            &api,
            projects.subscribe_registration_waits(),
            &input,
            &creation.project_id,
        )
        .await?;
        assert_eq!(
            pending.pointer("/result/result/kind"),
            Some(&json!("projectCreationPending")),
            "accepted creation became an error: {pending}"
        );
        assert_eq!(
            pending.pointer("/result/result/decision/project"),
            Some(&json!(creation.project_id))
        );
        assert!(!project_file.exists());
        assert_eq!(
            expire_registration_wait(
                &api,
                projects.subscribe_registration_waits(),
                &input,
                &creation.project_id,
            )
            .await?,
            pending,
            "same accepted pending request changed identity"
        );

        central.release()?;
        wait_welcome(ready.clone()).await?;
        timeout(BOUND, async {
            loop {
                let reply = api.call("project_create", input.clone()).await?;
                match reply.pointer("/result/result/kind").and_then(Value::as_str) {
                    Some("decided") => {
                        return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(reply);
                    }
                    Some("projectCreationPending") => tokio::task::yield_now().await,
                    _ => {
                        return Err(
                            format!("accepted request failed during readiness: {reply}").into()
                        );
                    }
                }
            }
        })
        .await??
    };
    assert_eq!(
        created.pointer("/result/result/decision/kind"),
        Some(&json!("accepted")),
        "{created}"
    );
    assert!(project_file.is_file());
    let replay = api.call("project_create", input).await?;
    assert_eq!(replay["result"]["result"], created["result"]["result"]);
    assert_eq!(fleet.accepted_creation_page("", 100).await?, vec![creation]);
    api.stop().await?;
    drop(fleet);
    running.shutdown().await?;
    drop(projects);
    Arc::try_unwrap(admission)
        .map_err(|_| io::Error::other("retained admission"))?
        .close()
        .await?;
    central.stop().await?;
    Ok(())
}

async fn expire_registration_wait(
    api: &ServedCollaborationApi,
    mut waits: tokio::sync::broadcast::Receiver<ProjectId>,
    input: &Value,
    expected_project: &ProjectId,
) -> FixtureResult<Value> {
    let creating = api.call("project_create", input.clone());
    tokio::pin!(creating);
    let project = timeout(BOUND, async {
        tokio::select! {
            result = &mut creating => {
                result?;
                FixtureResult::<ProjectId>::Err("creation settled before its registration wait".into())
            }
            project = waits.recv() => Ok(project?),
        }
    })
    .await??;
    if &project != expected_project {
        return Err("registration wait changed the exact project".into());
    }
    let pending_before_expiry = futures_util::poll!(creating.as_mut()).is_pending();
    // wait_ready is polled inside the owner's existing two-second timeout.
    tokio::time::pause();
    tokio::time::advance(std::time::Duration::from_millis(2001)).await;
    tokio::time::resume();
    let result = timeout(BOUND, creating).await??;
    if !pending_before_expiry {
        return Err("creation did not remain pending before controlled expiry".into());
    }
    Ok(result)
}

#[tokio::test]
async fn accepted_creation_past_the_ready_bound_returns_pending_then_local_and_replays()
-> TestResult {
    Box::pin(assert_creation_reply(false)).await
}

#[tokio::test]
async fn accepted_creation_waits_for_real_ready_inside_the_bound_and_replays() -> TestResult {
    Box::pin(assert_creation_reply(true)).await
}
