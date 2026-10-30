//! Real default loader, Unix API decoding, reload publication and mounted rendering.
use super::*;
use agent_session_inventory::source_inventory_request::SourceInventoryRejection;
use serde_json::json;

use agent_session_inventory::test_support::fake_api_support::{FakeCollaborationApi, FakeReply};

#[test]
fn rejected_default_catalogue_invalidates_provider_state_and_native_coverage_even_when_runtime_unknown()
 {
    use crate::session_picker::picker_model::SessionsPickerModel;
    use agent_session_inventory::picker_runtime_status::{NativeLiveCoverage, NativeLiveReadState};
    let summary=serde_json::from_value(json!({"origin":"hostedProvider","target":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"cursor-local"},"sessionId":"remembered-managed"},"workingDirectory":"/source/saved","updatedAt":2,"state":"running","requestedPolicy":{"access":"write-restricted"},"approver":{"humanId":"saved-approver"},"createdBy":{"humanId":"creator"}})).unwrap();
    let mut row = SessionPickerRecord::from_provider_summary(&summary, "Remembered Cursor");
    row.runtime_status = PickerRuntimeStatus::Unknown;
    let identity = row.identity.clone();
    let mut request = picker_request();
    request.records = vec![row];
    request.root = SessionsPickerRoot::Any;
    let mut model = SessionsPickerModel::new(request, 120);
    model.native_live_coverage = vec![NativeLiveCoverage {
        view: agent_session_inventory::picker_runtime_status::NativeCatalogueView::Loaded,
        endpoint: serde_json::from_value(
            json!({"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"claude-local"}),
        )
        .unwrap(),
        service_epoch: "00000000-0000-4000-8000-000000000002"
            .to_owned()
            .try_into()
            .unwrap(),
        state: NativeLiveReadState::Complete,
    }];
    model.invalidate_runtime_statuses();
    let row = model.request.records.first().unwrap();
    assert_eq!(row.identity, identity);
    assert_eq!(row.cwd.as_deref(), Some("/source/saved"));
    assert_eq!(row.runtime_status, PickerRuntimeStatus::Unknown);
    assert_eq!(row.provider_state, None);
    assert!(
        model.native_live_coverage.is_empty(),
        "a rejected read has no fresh native completeness claim"
    );
}

async fn create_stored_catalog(home: &Path) {
    use sqlx::Connection;
    std::fs::create_dir_all(home.join("sessions")).unwrap();
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(home.join("state_5.sqlite"))
        .create_if_missing(true);
    let mut connection = sqlx::SqliteConnection::connect_with(&options)
        .await
        .unwrap();
    for statement in [
        "CREATE TABLE threads (id TEXT PRIMARY KEY NOT NULL, rollout_path TEXT, created_at TEXT,
        updated_at TEXT, source TEXT, model_provider TEXT, cwd TEXT, name TEXT, title TEXT,
        sandbox_policy TEXT, approval_mode TEXT, tokens_used INTEGER, has_user_event INTEGER,
        archived INTEGER NOT NULL DEFAULT 0, archived_at TEXT, git_sha TEXT, git_branch TEXT,
        git_origin_url TEXT, cli_version TEXT, first_user_message TEXT NOT NULL DEFAULT '',
        agent_nickname TEXT, agent_role TEXT, memory_mode TEXT, model TEXT, reasoning_effort TEXT,
        agent_path TEXT, created_at_ms INTEGER, updated_at_ms INTEGER, thread_source TEXT,
        preview TEXT, recency_at TEXT, recency_at_ms INTEGER)",
        "CREATE INDEX idx_threads_created_at_ms ON threads(created_at_ms DESC, id DESC)",
        "CREATE INDEX idx_threads_updated_at_ms ON threads(updated_at_ms DESC, id DESC)",
        "INSERT INTO threads (id,cwd,source,thread_source,model_provider,first_user_message,created_at_ms,updated_at_ms)
        VALUES ('new-stored-row','/owned/project','cli','cli','codex-router','New independently stored row',1000,2000)",
    ] {sqlx::query(statement).execute(&mut connection).await.unwrap();}
    connection.close().await.unwrap();
}

#[tokio::test]
async fn default_host_invalid_inventory_is_rejected_and_remembered_rows_are_unknown() {
    for invalid in [true, false] {
        let home = tempfile::tempdir().unwrap();
        create_stored_catalog(home.path()).await;
        let mut api = FakeCollaborationApi::new(
            "00000000-0000-4000-8000-000000000001",
            "00000000-0000-4000-8000-000000000002",
        )
        .unwrap();
        // Both independent default inventory readers must decode the actual invalid reply.
        let peer = invalid.then(|| {
            api.serve(vec![
                FakeReply::Result(json!({"private":"DEFAULT_PRIVATE_SENTINEL"})),
                FakeReply::Result(json!({"private":"DEFAULT_PRIVATE_SENTINEL"})),
            ])
        });
        let directory = if invalid {
            api.directory().to_path_buf()
        } else {
            home.path().join("missing-host")
        };
        let actual =
            agent_session_inventory::session_picker_record_loader::session_picker_record_loader(
                Some(home.path().to_path_buf()),
                home.path().to_path_buf(),
                collaboration_client::session_catalog::discover_repository_identity(home.path()),
                Some(directory),
            );
        let completed = Arc::new(Mutex::new(None));
        let recorded = completed.clone();
        let loader: SessionsPickerRecordLoader = Arc::new(move |request| {
            let actual = actual.clone();
            let recorded = recorded.clone();
            Box::pin(async move {
                let result = actual(request).await;
                *recorded.lock().unwrap() = Some(matches!(
                    &result,
                    SourceInventoryResult::Rejected {
                        reason: SourceInventoryRejection::InvalidInventory,
                        ..
                    }
                ));
                result
            })
        });
        let mut request = picker_request();
        request.root = SessionsPickerRoot::Any;
        request.source = SessionsSource::All;
        request.records.truncate(1);
        request.records[0].runtime_status = PickerRuntimeStatus::Active;
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let input = futures_util::stream::unfold(receiver, |mut receiver| async {
            receiver.recv().await.map(|event| (event, receiver))
        });
        let mut outcome: Option<SessionsPickerOutcome> = None;
        let mut captured = None;
        let mut last = String::new();
        let render = async {
            let mut picker = element!(SessionsPickerComponent(request,record_loader:Some(loader),width:180usize,height:60usize,selected_outcome_out:&mut outcome));
            let frames = picker.mock_terminal_render_loop(MockTerminalConfig::with_events(input));
            tokio::pin!(frames);
            while let Some(canvas) = frames.next().await {
                let frame = canvas.to_string();
                if let Some(rejected) = *completed.lock().unwrap() {
                    assert_eq!(
                        rejected, invalid,
                        "invalid inventory must reject; missing service retains its existing availability behavior"
                    );
                    if invalid && frame.contains("This machine: Unavailable") {
                        assert!(
                            frame.contains("Feature design session"),
                            "previously published catalogue retained: {frame}"
                        );
                        assert!(
                            frame
                                .lines()
                                .any(|line| line.contains("Feature design session")
                                    && line.contains('?')),
                            "previous live claims invalidated: {frame}"
                        );
                        assert!(
                            !frame.contains("New independently stored row"),
                            "rejection cannot publish newly read rows: {frame}"
                        );
                        assert!(!frame.contains("Native Claude inventory incomplete"));
                        assert!(!frame.contains("DEFAULT_PRIVATE_SENTINEL"));
                        captured = Some(frame.clone());
                        sender.send(control_key('c')).unwrap();
                    } else if !invalid && frame.contains("Live status unavailable") {
                        assert!(!frame.contains("This machine: Unavailable"));
                        assert!(frame.contains("New independently stored row"));
                        captured = Some(frame.clone());
                        sender.send(control_key('c')).unwrap();
                    }
                }
                last = frame;
            }
        };
        assert!(
            tokio::time::timeout(Duration::from_secs(3), render)
                .await
                .is_ok(),
            "missing default publication: {last}"
        );
        assert!(captured.is_some());
        assert!(outcome.is_none());
        if let Some(peer) = peer {
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(1), peer)
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap()
                    .len(),
                2
            );
        }
    }
}
