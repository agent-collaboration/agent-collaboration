//! Actual child/decoder/service/MCP/loader/reload reaches the mounted captured Details.
use super::*;
use agent_session_inventory::sessions::{RouterRegistryRead, SessionPickerIdentity};
use agent_session_inventory::test_support::source_api_fixture::SourceApiFixture;
use agent_session_inventory::{
    source_inventory_request::InventoryPageProgress, source_inventory_request::PickerSourceContext,
    source_inventory_request::SourceInventoryResult,
};

fn key(code: KeyCode) -> TerminalEvent {
    TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
}

#[derive(Clone, Copy, Debug)]
enum NativeWarningCase {
    ConflictingPeers,
    InvalidJobs,
    UnavailableJobs,
}

#[tokio::test]
async fn actual_native_conflicts_and_invalid_jobs_reach_source_specific_picker_warnings() {
    const SERVICE: &str = "00000000-0000-4000-8000-000000000001";
    const SESSION: &str = "00000000-0000-4000-8000-000000000042";
    for warning_case in [
        NativeWarningCase::ConflictingPeers,
        NativeWarningCase::InvalidJobs,
        NativeWarningCase::UnavailableJobs,
    ] {
        let mut job = serde_json::json!({"kind":"background","id":"known-job","sessionId":SESSION,"cwd":"/source/native","name":"Known native job","state":"blocked","startedAt":1790162100000_i64});
        if matches!(warning_case, NativeWarningCase::InvalidJobs) {
            job.as_object_mut().unwrap().remove("state");
        }
        let source =
            SourceApiFixture::start_with_native_jobs(SERVICE, "source-model", vec![job]).await;
        let mut live_claim = tokio::process::Command::new("/bin/cat")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        if matches!(warning_case, NativeWarningCase::ConflictingPeers) {
            source.publish_native_peer(SESSION, "/source/current");
            source.duplicate_native_peer_claim(live_claim.id().unwrap());
        }
        if matches!(warning_case, NativeWarningCase::UnavailableJobs) {
            source.fail_native_jobs_after_history();
        }
        let PickerSourceContext::ConfiguredHosted(profile) =
            source.request("Observed source").source_context
        else {
            panic!("configured source")
        };
        let mut request = picker_request();
        request.root = SessionsPickerRoot::Any;
        request.source = SessionsSource::All;
        request.records.clear();
        request.router_registry = RouterRegistryRead::Ready(agent_session_inventory::sessions::router_connection_registry::RouterConnectionRegistry { routers: vec![profile] });
        let actual = source.loader();
        let loader: SessionsPickerRecordLoader = Arc::new(move |request| {
            let actual = actual.clone();
            Box::pin(async move {
                if request.source_context == PickerSourceContext::DefaultHosted {
                    SourceInventoryResult::Ready {
                        request,
                        binding: None,
                        progress: InventoryPageProgress::Complete,
                        snapshot: observed_records(Vec::new()),
                    }
                } else {
                    actual(request).await
                }
            })
        });
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        sender.send(key(KeyCode::F(2))).unwrap();
        let events = futures_util::stream::unfold(receiver, |mut receiver| async {
            receiver.recv().await.map(|event| (event, receiver))
        });
        let mut phase = 0;
        let mut last = String::new();
        let mut picker = element!(SessionsPickerComponent(request,record_loader:Some(loader),width:132usize,height:60usize));
        let frames = picker.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
        tokio::pin!(frames);
        let journey = async {
            while let Some(canvas) = frames.next().await {
                let frame = canvas.to_string();
                if phase == 0 && frame.contains("Choose machine") {
                    sender.send(key(KeyCode::Enter)).unwrap();
                    sender.send(key(KeyCode::Down)).unwrap();
                    sender.send(key(KeyCode::Enter)).unwrap();
                    phase = 1;
                } else if phase == 1
                    && matches!(warning_case, NativeWarningCase::ConflictingPeers)
                    && frame.contains("multiple live peers claim this session")
                {
                    assert!(
                        frame.contains(SERVICE)
                            && frame.contains(SESSION)
                            && frame.contains("claude-local"),
                        "source and session must accompany the reason: {frame}"
                    );
                    assert!(
                        frame.contains("Known native job"),
                        "independent job facts remain visible: {frame}"
                    );
                    sender.send(ctrl_key('c')).unwrap();
                    phase = 2;
                } else if phase == 1
                    && matches!(warning_case, NativeWarningCase::InvalidJobs)
                    && frame.contains("native Claude background jobs inventory: invalid output")
                {
                    assert!(
                        frame.contains("Observed source: Unavailable"),
                        "failure must name the configured source: {frame}"
                    );
                    assert!(!frame.contains("session inventory does not match its source"));
                    sender.send(ctrl_key('c')).unwrap();
                    phase = 2;
                } else if phase == 1
                    && matches!(warning_case, NativeWarningCase::UnavailableJobs)
                    && frame.contains("Observed source")
                    && frame.contains("Native Claude inventory incomplete")
                {
                    assert!(
                        frame.contains("Native Claude background jobs: read unavailable"),
                        "partial native read must identify its source and reason: {frame}"
                    );
                    assert!(frame.contains(SERVICE) && frame.contains("claude-local"));
                    assert!(frame.contains("Known native job"));
                    sender.send(ctrl_key('c')).unwrap();
                    phase = 2;
                }
                last = frame;
            }
        };
        assert!(
            tokio::time::timeout(Duration::from_secs(5), journey)
                .await
                .is_ok(),
            "missing native warning, case={warning_case:?}, phase={phase}: {last}"
        );
        assert_eq!(phase, 2);
        live_claim.kill().await.unwrap();
        source.finish().await;
    }
}

#[tokio::test]
async fn actual_history_job_and_peer_only_loaded_observation_keep_both_facts() {
    const SESSION: &str = "00000000-0000-4000-8000-000000000042";
    let source=SourceApiFixture::start_with_native_jobs("00000000-0000-4000-8000-000000000001","source-model",vec![serde_json::json!({"kind":"background","id":"history-job-handle","sessionId":SESSION,"cwd":"/source/native","name":"History job","state":"stopped","startedAt":1790162100000_i64})]).await;
    source.publish_native_peer(SESSION, "/source/native");
    source.fail_native_jobs_after_history();
    let snapshot =
        agent_session_inventory::test_support::complete_source_reader::read_complete_source(
            source.loader(),
            source.request("Observed source"),
        )
        .await
        .into_snapshot()
        .unwrap();
    let native = snapshot
        .records
        .iter()
        .filter(|row| matches!(row.identity, SessionPickerIdentity::NativeClaude(_)))
        .collect::<Vec<_>>();
    assert_eq!(native.len(), 1);
    let row = native[0];
    let job = row.native_background.as_ref().unwrap();
    assert_eq!(String::from(job.job_handle.clone()), "history-job-handle");
    assert_eq!(row.recency_at_ms, Some(1790162200000_i64));
    assert_eq!(
        row.native_peer.as_ref().map(|peer| peer.updated_at),
        Some(1790162200_i64),
        "Loaded peer facts survive when only Stored had the job"
    );
    assert!(
        snapshot
            .native_live_coverage
            .iter()
            .any(|part| part.state.is_incomplete())
    );
    source.finish().await;
}

#[tokio::test]
async fn actual_working_job_stays_active_with_idle_or_unreported_peer_metadata() {
    const SESSION: &str = "00000000-0000-4000-8000-000000000042";
    for status in [Some("idle"), None] {
        let source=SourceApiFixture::start_with_native_jobs("00000000-0000-4000-8000-000000000001","source-model",vec![serde_json::json!({"kind":"background","id":"active-job-handle","sessionId":SESSION,"cwd":"/source/native","name":"Working despite peer metadata","state":"working","startedAt":1790162100000_i64})]).await;
        source.publish_native_peer_with_status(SESSION, "/source/native", status);
        let snapshot =
            agent_session_inventory::test_support::complete_source_reader::read_complete_source(
                source.loader(),
                source.request("Observed source"),
            )
            .await
            .into_snapshot()
            .unwrap();
        let row = snapshot
            .records
            .iter()
            .find(|row| row.session_id == SESSION)
            .unwrap();
        assert_eq!(
            row.runtime_status,
            agent_session_inventory::picker_runtime_status::PickerRuntimeStatus::Active,
            "known working evidence cannot disappear under Active: {status:?}"
        );
        let job = row.native_background.as_ref().unwrap();
        assert_eq!(
            job.state,
            collaboration_client::protocol::NativeClaudeBackgroundState::Working
        );
        assert_eq!(
            row.native_peer.as_ref().unwrap().status,
            if status.is_some() {
                collaboration_client::protocol::ClaudeCodeInteractiveStatus::Idle
            } else {
                collaboration_client::protocol::ClaudeCodeInteractiveStatus::Unreported
            }
        );
        source.finish().await;
    }
}

#[tokio::test]
async fn actual_native_peer_corroboration_keeps_actual_updated_time_and_job_identity() {
    const SESSION: &str = "00000000-0000-4000-8000-000000000042";
    let source=SourceApiFixture::start_with_native_jobs("00000000-0000-4000-8000-000000000001","source-model",vec![serde_json::json!({"kind":"background","id":"distinct-job-handle","sessionId":SESSION,"cwd":"/source/native","name":"Corroborated job","state":"working","startedAt":1790162100000_i64})]).await;
    source.publish_native_peer(SESSION, "/source/resumed");
    let snapshot =
        agent_session_inventory::test_support::complete_source_reader::read_complete_source(
            source.loader(),
            source.request("Observed source"),
        )
        .await
        .into_snapshot()
        .unwrap();
    let native = snapshot
        .records
        .iter()
        .filter(|row| matches!(row.identity, SessionPickerIdentity::NativeClaude(_)))
        .collect::<Vec<_>>();
    assert_eq!(native.len(), 1);
    let row = native[0];
    assert_eq!(
        row.recency_at_ms,
        Some(1790162200000_i64),
        "only actual peer Updated may populate recency"
    );
    assert_eq!(row.created_at_ms, Some(1790162100000_i64));
    assert_eq!(row.session_id, SESSION);
    let job = row.native_background.as_ref().unwrap();
    assert_eq!(String::from(job.job_handle.clone()), "distinct-job-handle");
    assert_eq!(
        String::from(job.working_directory.clone()),
        "/source/native"
    );
    assert_eq!(row.cwd.as_deref(), Some("/source/resumed"));
    assert_eq!(row.native_peer.as_ref().unwrap().updated_at, 1790162200_i64);
    source.finish().await;
}

#[tokio::test]
async fn actual_native_api_distinguishes_complete_empty_unidentified_and_invalid_jobs() {
    use agent_session_inventory::picker_runtime_status::NativeLiveReadState;
    for (jobs, incomplete, invalid) in [
        (Vec::new(), false, false),
        (
            vec![
                serde_json::json!({"kind":"background","id":"unidentified-handle","cwd":"/native","state":"blocked","startedAt":1790162100000_i64}),
            ],
            true,
            false,
        ),
        (
            vec![
                serde_json::json!({"kind":"background","id":"invalid-handle","cwd":"/native","startedAt":1790162100000_i64}),
            ],
            false,
            true,
        ),
    ] {
        let source = SourceApiFixture::start_with_native_jobs(
            "00000000-0000-4000-8000-000000000001",
            "source-model",
            jobs,
        )
        .await;
        let result =
            agent_session_inventory::test_support::complete_source_reader::read_complete_source(
                source.loader(),
                source.request("Observed source"),
            )
            .await
            .into_snapshot();
        if invalid {
            assert!(
                result.is_err(),
                "partial job discriminator rejects whole source"
            );
        } else {
            let snapshot =
                result.expect("qualified managed rows survive unavailable native identity");
            assert_eq!(snapshot.records.len(), 2);
            assert_eq!(
                snapshot.native_live_coverage.len(),
                2,
                "history and live coverage remain independent"
            );
            for part in snapshot.native_live_coverage {
                assert_eq!(part.state.is_incomplete(), incomplete);
                if incomplete {
                    assert!(matches!(part.state, NativeLiveReadState::Incomplete(_)));
                } else {
                    assert_eq!(part.state, NativeLiveReadState::Complete);
                }
            }
        }
        source.finish().await;
    }
}

#[tokio::test]
async fn actual_native_history_and_loaded_views_corroborate_once_without_inventing_updated_time() {
    let jobs = [
        (
            "00000000-0000-4000-8000-000000000041",
            "working",
            "Live background",
        ),
        (
            "00000000-0000-4000-8000-000000000042",
            "stopped",
            "Terminal background",
        ),
        (
            "00000000-0000-4000-8000-000000000043",
            "future-state",
            "Unknown background",
        ),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (session, state, name))| {
        serde_json::json!({
            "kind":"background","id":format!("job-{index}"),"sessionId":session,
            "cwd":"/source/native","name":name,"state":state,"startedAt":1790162100000_i64
        })
    })
    .collect();
    let source = SourceApiFixture::start_with_native_jobs(
        "00000000-0000-4000-8000-000000000001",
        "source-model",
        jobs,
    )
    .await;
    let result =
        agent_session_inventory::test_support::complete_source_reader::read_complete_source(
            source.loader(),
            source.request("Observed source"),
        )
        .await;
    let snapshot = result
        .into_snapshot()
        .expect("real native history must publish");
    let native = snapshot
        .records
        .iter()
        .filter(|row| matches!(row.identity, SessionPickerIdentity::NativeClaude(_)))
        .collect::<Vec<_>>();
    assert_eq!(
        native.len(),
        3,
        "Stored and Loaded corroboration must yield one row per native UUID"
    );
    for row in &native {
        assert!(
            row.recency_at_ms.is_none(),
            "job start cannot invent Updated: {row:?}"
        );
        assert_eq!(row.cwd.as_deref(), Some("/source/native"));
        assert!(row.native_background.is_some());
    }
    assert_eq!(
        native
            .iter()
            .find(|row| row.title == "Live background")
            .unwrap()
            .runtime_status,
        agent_session_inventory::picker_runtime_status::PickerRuntimeStatus::Active
    );
    assert_eq!(
        native
            .iter()
            .find(|row| row.title == "Terminal background")
            .unwrap()
            .runtime_status,
        agent_session_inventory::picker_runtime_status::PickerRuntimeStatus::NotLoaded
    );
    assert_eq!(
        native
            .iter()
            .find(|row| row.title == "Unknown background")
            .unwrap()
            .runtime_status,
        agent_session_inventory::picker_runtime_status::PickerRuntimeStatus::Unknown
    );
    source.finish().await;
}

#[tokio::test]
async fn actual_native_background_api_reaches_captured_details_and_copy_as_uuid() {
    const SESSION: &str = "00000000-0000-4000-8000-000000000042";
    let source=SourceApiFixture::start_with_native_jobs("00000000-0000-4000-8000-000000000001","source-model",vec![serde_json::json!({"kind":"background","id":"distinct-job-handle","sessionId":SESSION,"cwd":"/source/native","name":"Before prompt native","state":"blocked","startedAt":1790162100000_i64})]).await;
    let PickerSourceContext::ConfiguredHosted(mut profile) =
        source.request("Observed source").source_context
    else {
        panic!("configured source")
    };
    profile.name = "Observed source".to_owned().try_into().unwrap();
    let mut request = picker_request();
    request.root = SessionsPickerRoot::Any;
    request.source = SessionsSource::All;
    request.records.clear();
    request.router_registry = RouterRegistryRead::Ready(
        agent_session_inventory::sessions::router_connection_registry::RouterConnectionRegistry {
            routers: vec![profile],
        },
    );
    let actual = source.loader();
    let loader: SessionsPickerRecordLoader = Arc::new(move |request| {
        let actual = actual.clone();
        Box::pin(async move {
            if request.source_context == PickerSourceContext::DefaultHosted {
                SourceInventoryResult::Ready {
                    request,
                    binding: None,
                    progress: InventoryPageProgress::Complete,
                    snapshot: observed_records(Vec::new()),
                }
            } else {
                actual(request).await
            }
        })
    });
    let copied = Arc::new(Mutex::new(Vec::<String>::new()));
    let copier: crate::session_picker::session_id_clipboard::SessionClipboardWriter = Arc::new({
        let copied = copied.clone();
        move |session_id| {
            copied.lock().unwrap().push(session_id.to_owned());
            Ok(())
        }
    });
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    sender.send(key(KeyCode::F(2))).unwrap();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut outcome: Option<SessionsPickerOutcome> = None;
    let mut phase = 0;
    let mut last = String::new();
    {
        let mut picker = element!(SessionsPickerComponent(request,record_loader:Some(loader),session_clipboard_writer:Some(copier),width:180usize,height:60usize,selected_outcome_out:&mut outcome));
        let frames = picker.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
        tokio::pin!(frames);
        let journey = async {
            while let Some(canvas) = frames.next().await {
                let frame = canvas.to_string();
                if phase == 0 && frame.contains("Choose machine") {
                    sender.send(key(KeyCode::Enter)).unwrap();
                    sender.send(key(KeyCode::Down)).unwrap();
                    sender.send(key(KeyCode::Enter)).unwrap();
                    phase = 1;
                } else if phase == 1
                    && frame.contains("Observed source: 3 sessions")
                    && frame.contains("Before prompt native")
                {
                    assert!(frame.contains("Source provider"));
                    let (row, line) = frame
                        .lines()
                        .enumerate()
                        .find(|(_, line)| line.contains("Before prompt native"))
                        .unwrap();
                    let column = unicode_width::UnicodeWidthStr::width(
                        line.get(..line.find("Before prompt native").unwrap())
                            .unwrap(),
                    );
                    sender
                        .send(TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                            MouseEventKind::Moved,
                            u16::try_from(column)
                                .expect("captured fixture coordinates fit terminal cells"),
                            u16::try_from(row)
                                .expect("captured fixture coordinates fit terminal cells"),
                        )))
                        .unwrap();
                    phase = 2;
                } else if phase == 2 && frame.contains("❯ Before prompt native") {
                    sender.send(key(KeyCode::F(4))).unwrap();
                    phase = 3;
                } else if phase == 3 && frame.contains("Session details") {
                    assert!(frame.contains(SESSION), "native UUID retained: {frame}");
                    assert!(
                        frame.contains("Job handle: distinct-job-handle"),
                        "handle must remain a separately labeled fact: {frame}"
                    );
                    assert!(frame.contains("Machine: Observed source"), "{frame}");
                    assert!(
                        frame.contains("Job state: Blocked"),
                        "job lifecycle remains separately visible: {frame}"
                    );
                    sender.send(key(KeyCode::Enter)).unwrap();
                    phase = 4;
                } else if phase == 4 && frame.contains("Copy requested") {
                    sender.send(ctrl_key('d')).unwrap();
                    phase = 5;
                }
                last = frame;
            }
        };
        assert!(
            tokio::time::timeout(Duration::from_secs(4), journey)
                .await
                .is_ok(),
            "native publication phase={phase}: {last}"
        );
    }
    assert_eq!(phase, 5);
    assert_eq!(copied.lock().unwrap().as_slice(), [SESSION]);
    assert!(outcome.is_none());
    source.finish().await;
}
