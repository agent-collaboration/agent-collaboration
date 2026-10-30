//! Existing static list and directory lookup consume the actual native catalogue.
use super::*;

#[tokio::test]
async fn native_background_active_list_and_resume_directory_survive_public_api_projection() {
    use claude_code_peer_messaging::{ClaudeCodeSessionRegistry, NativeClaudeJobReader};
    use collaboration_service::ApplicationFixture;
    for (corroborated, background_available, conflicted) in [
        (true, true, false),
        (false, true, false),
        (true, false, false),
        (true, true, true),
    ] {
        let root = tempfile::tempdir().unwrap();
        let service_directory = root.path().join("agent-communication");
        fs::create_dir(&service_directory).unwrap();
        fs::set_permissions(&service_directory, fs::Permissions::from_mode(0o700)).unwrap();
        let observed_directory = root.path().join("observed project");
        fs::create_dir(&observed_directory).unwrap();
        let peer_directory = root.path().join("resumed project");
        fs::create_dir(&peer_directory).unwrap();
        let peers = root.path().join("peers");
        fs::create_dir(&peers).unwrap();
        if corroborated {
            fs::write(peers.join(format!("{}.json",std::process::id())),serde_json::to_vec(&serde_json::json!({"pid":std::process::id(),"sessionId":SESSION_ID,"name":"Observed busy peer","cwd":peer_directory,"status":"busy","startedAt":1760000000000_i64,"updatedAt":1760000001000_i64,"statusUpdatedAt":1760000001000_i64,"kind":"interactive","entrypoint":"cli","peerProtocol":1,"messagingSocketPath":peers.join("peer.sock")})).unwrap()).unwrap();
        }
        let mut duplicate_process = if conflicted {
            let child = tokio::process::Command::new("/bin/cat")
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::null())
                .kill_on_drop(true)
                .spawn()
                .unwrap();
            let mut peer: serde_json::Value = serde_json::from_slice(
                &fs::read(peers.join(format!("{}.json", std::process::id()))).unwrap(),
            )
            .unwrap();
            peer["pid"] = serde_json::json!(child.id().unwrap());
            fs::write(
                peers.join(format!("{}.json", child.id().unwrap())),
                serde_json::to_vec(&peer).unwrap(),
            )
            .unwrap();
            Some(child)
        } else {
            None
        };
        let jobs = root.path().join("jobs.json");
        fs::write(&jobs,serde_json::to_vec(&serde_json::json!([{"kind":"background","id":"separate-background-handle","sessionId":SESSION_ID,"cwd":observed_directory,"name":"Active native background","state":"working","startedAt":1760000000000_i64}])).unwrap()).unwrap();
        let executable = root.path().join("native-list");
        node_test_fixture::write_node_fixture(
            &executable,
            "native-cli-catalogue-agent.ts",
            &serde_json::json!({"kind":"file","output":{"kind":"path","path":jobs}}),
        )
        .unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        let description=serde_json::from_value(serde_json::json!({"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"claude-local"},"label":"Native Claude","availability":{"state":"unavailable","observedAt":"2026-10-18T00:00:00Z","reason":"ACP unavailable"},"channels":[]})).unwrap();
        let mut identity = ApplicationFixture::new(
            "00000000-0000-4000-8000-000000000001",
            "00000000-0000-4000-8000-000000000002",
        )
        .unwrap()
        .with_endpoints(vec![description])
        .unwrap()
        .with_claude_code_sessions(std::sync::Arc::new(ClaudeCodeSessionRegistry::new(peers)));
        if background_available {
            identity = identity.with_claude_background_jobs(std::sync::Arc::new(
                NativeClaudeJobReader::for_executable(executable),
            ));
        }
        let api = collaboration_mcp::test_support::ServedCollaborationApi::start(
            &service_directory,
            (identity).build(),
        )
        .unwrap();
        let target = ClaudeLaunchTarget {
            service_directory,
            secret_root: root.path().join("secrets"),
            claude_projects_directory: None,
        };
        let (records, error) = target.session_records(10).await.unwrap();
        assert_eq!(
            error.is_some(),
            !background_available || conflicted,
            "an unavailable background read must be visible while preserving the peer: {error:?}"
        );
        assert_eq!(
            records.len(),
            1,
            "native Background must not drop a formerly visible busy peer or job"
        );
        assert_eq!(records[0]["source"], "active");
        assert_eq!(records[0]["sessionId"], SESSION_ID);
        assert_eq!(
            records[0]["workingDirectory"],
            serde_json::json!(if background_available {
                &observed_directory
            } else {
                &peer_directory
            })
        );
        if background_available {
            assert_eq!(records[0]["jobHandle"], "separate-background-handle");
        } else {
            assert!(records[0].get("jobHandle").is_none());
        }
        assert_eq!(
            records[0]["updatedAtUnixSeconds"],
            if corroborated && !conflicted {
                serde_json::json!(1760000001_i64)
            } else {
                serde_json::Value::Null
            }
        );
        let resumed_directory = target.resume_working_directory(SESSION_ID).await;
        if conflicted {
            assert_eq!(records[0]["nativeConflict"]["kind"], "duplicateLiveClaims");
            let error = resumed_directory.unwrap_err().to_string();
            assert!(
                error.contains(SESSION_ID)
                    && error.contains("claude-local")
                    && error.contains("multiple live peers claim this session")
            );
        } else {
            assert_eq!(
                resumed_directory.unwrap(),
                Some(if corroborated {
                    peer_directory
                } else {
                    observed_directory
                })
            );
        }
        if let Some(child) = &mut duplicate_process {
            child.kill().await.unwrap();
        }
        api.stop().await.unwrap();
    }
}
