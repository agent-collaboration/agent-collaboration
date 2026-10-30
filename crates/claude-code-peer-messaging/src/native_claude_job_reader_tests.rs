//! Vendor-shaped fixtures cross the actual child/decoder; no vendor is invoked.
use super::*;
use node_test_fixture as node_fixture;
use serde_json::{Value, json};

fn job(state: &str) -> Value {
    json!({"kind":"background","id":"job-handle-42","sessionId":"00000000-0000-4000-8000-000000000042","cwd":"/source/project","startedAt":1790162100123_i64,"state":state,"name":"Before first prompt"})
}

#[test]
fn claude_2_1_312_mixed_catalogue_preserves_jobs_without_requiring_peer_job_fields() {
    // Recorded, redacted vendor output includes peer-only rows and waitingFor.
    let fixture = include_bytes!("test_fixtures/claude_agents_2_1_312.json");

    let observed = decode_background_jobs(fixture)
        .expect("Claude 2.1.312 interactive rows must not invalidate background inventory");

    assert_eq!(observed.jobs.len(), 10);
    assert_eq!(observed.unidentified_jobs, 0);
    assert!(
        observed
            .jobs
            .iter()
            .take(9)
            .all(|job| job.state == NativeClaudeBackgroundState::Blocked)
    );
    let working = observed.jobs.last().unwrap();
    assert_eq!(String::from(working.job_handle.clone()), "00000010");
    assert_eq!(
        String::from(working.session_id.clone()),
        "00000000-0000-4000-8000-000000000010"
    );
    assert_eq!(working.state, NativeClaudeBackgroundState::Working);
    assert_eq!(working.status, Some(ClaudeCodeInteractiveStatus::Idle));
    assert_eq!(working.started_at, 1_793_291_703);
}

#[test]
fn claude_2_1_312_all_catalogue_accepts_missing_names_and_terminal_states() {
    let fixture = include_bytes!("test_fixtures/claude_agents_all_2_1_312.json");

    let observed = decode_background_jobs(fixture)
        .expect("the recorded complete Claude 2.1.312 catalogue must decode");

    assert_eq!(observed.jobs.len(), 26);
    assert_eq!(observed.unidentified_jobs, 0);
    assert_eq!(
        observed
            .jobs
            .iter()
            .filter(|job| job.name.is_none())
            .count(),
        2
    );
    for (state, count) in [
        (NativeClaudeBackgroundState::Blocked, 9),
        (NativeClaudeBackgroundState::Working, 1),
        (NativeClaudeBackgroundState::Done, 12),
        (NativeClaudeBackgroundState::Failed, 3),
        (NativeClaudeBackgroundState::Stopped, 1),
    ] {
        assert_eq!(
            observed
                .jobs
                .iter()
                .filter(|job| job.state == state)
                .count(),
            count
        );
    }
    let working = observed.jobs.last().unwrap();
    assert_eq!(String::from(working.job_handle.clone()), "00000026");
    assert_eq!(working.status, Some(ClaudeCodeInteractiveStatus::Busy));
}

fn executable_fixture(configuration: Value) -> (tempfile::TempDir, NativeClaudeJobReader) {
    let root = tempfile::tempdir().unwrap();
    let executable = root.path().join("claude");
    node_fixture::write_node_fixture(&executable, "native-cli-catalogue-agent.ts", &configuration)
        .unwrap();
    let reader = NativeClaudeJobReader::for_executable(executable);
    (root, reader)
}

#[tokio::test]
async fn native_job_only_before_prompt_and_mixed_peer_rows_remain_distinct() {
    for peers in [false, true] {
        let mut list = vec![job("blocked"), {
            let mut stopped = job("stopped");
            stopped["id"] = json!("stopped-job");
            stopped["sessionId"] = json!("00000000-0000-4000-8000-000000000043");
            stopped
        }];
        if peers {
            for kind in ["interactive", "background"] {
                list.push(json!({"kind":kind,"pid":123,"cwd":"/peer/project","startedAt":1790162100123_i64,"status":"idle"}));
            }
        }
        let text = serde_json::to_string(&list).unwrap();
        let (_root, reader) = executable_fixture(json!({"kind":"text","text":text}));
        let observed = reader
            .read_background_jobs()
            .await
            .expect("supported job-only and mixed native JSON must decode");
        assert_eq!(
            observed.jobs.len(),
            2,
            "peer-only rows neither poison nor fabricate jobs"
        );
        assert_eq!(observed.unidentified_jobs, 0);
        let first = observed.jobs.first().unwrap();
        assert_eq!(String::from(first.job_handle.clone()), "job-handle-42");
        assert_eq!(
            String::from(first.session_id.clone()),
            "00000000-0000-4000-8000-000000000042"
        );
        assert_eq!(first.started_at, 1790162100);
        assert_eq!(first.state, NativeClaudeBackgroundState::Blocked);
        assert_eq!(
            observed.jobs.get(1).unwrap().state,
            NativeClaudeBackgroundState::Stopped
        );
    }
}

#[test]
fn native_job_decoder_preserves_unknown_state_and_counts_only_missing_native_uuid() {
    let mut unidentified = job("blocked");
    unidentified.as_object_mut().unwrap().remove("sessionId");
    let result = decode_background_jobs(&serde_json::to_vec(&vec![unidentified]).unwrap()).unwrap();
    assert!(result.jobs.is_empty());
    assert_eq!(result.unidentified_jobs, 1);
    let unknown =
        decode_background_jobs(&serde_json::to_vec(&vec![job("future-paused")]).unwrap()).unwrap();
    let state = &unknown.jobs.first().unwrap().state;
    assert!(
        matches!(state,NativeClaudeBackgroundState::Unknown {state} if String::from(state.clone())=="future-paused")
    );
    assert!(!state.is_loaded());
    assert!(!state.is_active());
    assert!(!state.is_terminal());
    for (label, loaded, active, terminal) in [
        ("working", true, true, false),
        ("blocked", true, false, false),
        ("done", false, false, true),
        ("failed", false, false, true),
        ("stopped", false, false, true),
    ] {
        let decoded =
            decode_background_jobs(&serde_json::to_vec(&vec![job(label)]).unwrap()).unwrap();
        let state = &decoded.jobs.first().unwrap().state;
        assert_eq!(
            (state.is_loaded(), state.is_active(), state.is_terminal()),
            (loaded, active, terminal)
        );
    }
}

#[test]
fn malformed_job_fields_and_conflicting_job_identities_reject_instead_of_becoming_empty() {
    for (field, value) in [
        ("sessionId", json!("opaque-handle-is-not-native-uuid")),
        ("id", json!("")),
        ("cwd", json!("relative")),
        ("startedAt", json!("not-a-time")),
        ("startedAt", Value::Null),
        ("state", json!("")),
        ("state", json!("x".repeat(129))),
    ] {
        let mut row = job("blocked");
        row[field] = value;
        assert_eq!(
            decode_background_jobs(&serde_json::to_vec(&vec![row]).unwrap()),
            Err(NativeClaudeJobReadError::InvalidOutput),
            "{field}"
        );
    }
    for missing in ["id", "state"] {
        let mut row = job("blocked");
        row.as_object_mut().unwrap().remove(missing);
        assert_eq!(
            decode_background_jobs(&serde_json::to_vec(&vec![row]).unwrap()),
            Err(NativeClaudeJobReadError::InvalidOutput),
            "partial job discriminator cannot become a peer"
        );
    }
    for different in ["id", "sessionId"] {
        let mut second = job("blocked");
        second[different] = json!(if different == "id" {
            "different-handle"
        } else {
            "00000000-0000-4000-8000-000000000099"
        });
        assert_eq!(
            decode_background_jobs(&serde_json::to_vec(&vec![job("blocked"), second]).unwrap()),
            Err(NativeClaudeJobReadError::InvalidOutput)
        );
    }
    for bytes in [b"{}".as_slice(), b"{broken".as_slice(), b"null".as_slice()] {
        assert_eq!(
            decode_background_jobs(bytes),
            Err(NativeClaudeJobReadError::InvalidOutput)
        );
    }
}

#[tokio::test]
async fn native_read_failure_and_bounded_output_do_not_expose_private_diagnostics() {
    let (_root, reader) =
        executable_fixture(json!({"kind":"failure","stderr":"PRIVATE_JOB_DIAGNOSTIC","exit":7}));
    let error = reader.read_background_jobs().await.unwrap_err();
    assert_eq!(
        error,
        NativeClaudeJobReadError::Unavailable(NativeInventoryUnavailableReason::ReadUnavailable)
    );
    assert!(!error.to_string().contains("PRIVATE_JOB_DIAGNOSTIC"));
    for stderr in [true, false] {
        let configuration =
            json!({"kind":"overflow","stream":if stderr {"stderr"} else {"stdout"},"bytes":256});
        let (_root, mut reader) = executable_fixture(configuration);
        reader.stderr_limit = 32;
        reader.stdout_limit = 32;
        assert_eq!(
            reader.read_background_jobs().await,
            Err(NativeClaudeJobReadError::InvalidOutput)
        );
    }
    let root = tempfile::tempdir().unwrap();
    let reader =
        NativeClaudeJobReader::for_executable(root.path().join("missing-native-executable"));
    assert_eq!(
        reader.read_background_jobs().await,
        Err(NativeClaudeJobReadError::Unavailable(
            NativeInventoryUnavailableReason::ExecutableUnavailable
        ))
    );
}

#[test]
fn identical_jobs_and_malformed_peer_only_background_rows_are_invalid() {
    assert_eq!(
        decode_background_jobs(&serde_json::to_vec(&vec![job("blocked"), job("blocked")]).unwrap()),
        Err(NativeClaudeJobReadError::InvalidOutput),
        "identical rows are not silently deduplicated"
    );
    let peer = json!({"kind":"background","pid":123,"cwd":"/peer/project","startedAt":1790162100123_i64,"status":"idle"});
    for (field, value) in [
        ("pid", None),
        ("pid", Some(json!(0))),
        ("pid", Some(json!("123"))),
        ("cwd", Some(json!("relative"))),
        ("startedAt", Some(json!("invalid"))),
        ("status", Some(json!(7))),
    ] {
        let mut invalid = peer.clone();
        if let Some(value) = value {
            invalid[field] = value;
        } else {
            invalid.as_object_mut().unwrap().remove(field);
        }
        assert_eq!(
            decode_background_jobs(&serde_json::to_vec(&vec![invalid]).unwrap()),
            Err(NativeClaudeJobReadError::InvalidOutput),
            "peer-only background {field} must validate before it is skipped"
        );
    }
}

#[tokio::test]
async fn cancelling_or_timing_out_native_inventory_settles_only_its_owned_child() {
    use tokio::io::{AsyncBufReadExt, BufReader};
    for cancel in [true, false] {
        let socket_root = tempfile::tempdir().unwrap();
        let socket_path = socket_root.path().join("ready.sock");
        let listener = tokio::net::UnixListener::bind(&socket_path).unwrap();
        let diagnostic = socket_root.path().join("fixture-error.txt");
        let configuration =
            json!({"kind":"socket","socketPath":socket_path,"diagnosticPath":diagnostic});
        let (_root, mut reader) = executable_fixture(configuration);
        // Process startup is inside the read budget; concurrent cold Node startup exceeded 500ms.
        reader.timeout = Duration::from_secs(2);
        let task = tokio::spawn(async move { reader.read_background_jobs().await });
        let accepted = tokio::time::timeout(Duration::from_secs(3), listener.accept()).await;
        let (stream, _) = match accepted {
            Ok(accepted) => accepted.unwrap(),
            Err(error) => panic!(
                "fixture did not reach owned socket: {error}; read={:?}; diagnostic={:?}",
                task.await,
                std::fs::read_to_string(diagnostic)
            ),
        };
        let mut stream = BufReader::new(stream);
        let mut process_id = String::new();
        tokio::time::timeout(Duration::from_secs(1), stream.read_line(&mut process_id))
            .await
            .unwrap()
            .unwrap();
        let process_id =
            rustix::process::Pid::from_raw(process_id.trim().parse::<i32>().unwrap()).unwrap();
        if cancel {
            task.abort();
            assert!(task.await.unwrap_err().is_cancelled());
        } else {
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(3), task)
                    .await
                    .unwrap()
                    .unwrap(),
                Err(NativeClaudeJobReadError::Unavailable(
                    NativeInventoryUnavailableReason::ReadTimedOut
                ))
            );
        }
        let mut remainder = Vec::new();
        tokio::time::timeout(Duration::from_secs(1), stream.read_to_end(&mut remainder))
            .await
            .unwrap()
            .unwrap();
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                match rustix::process::test_kill_process(process_id) {
                    Err(rustix::io::Errno::SRCH) => break,
                    Ok(()) => tokio::task::yield_now().await,
                    Err(error) => panic!("owned child observation failed: {error}"),
                }
            }
        })
        .await
        .expect("only the reader's child must be killed and reaped");
    }
}
