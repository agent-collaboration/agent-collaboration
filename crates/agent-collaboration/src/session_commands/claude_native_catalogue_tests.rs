//! Existing static list and directory lookup consume the actual native catalogue.
use super::*;

#[tokio::test]
async fn native_background_active_list_and_resume_directory_survive_public_api_projection() {
    use claude_code_peer_messaging::{ClaudeCodeSessionRegistry, NativeClaudeJobReader};
    use collaboration_service::{CollaborationApplication, ServiceIdentity};
    for (corroborated, background_available) in [(true, true), (false, true), (true, false)] {
        let root = tempfile::tempdir().unwrap();
        let service_directory = root.path().join("agent-communication");
        fs::create_dir(&service_directory).unwrap();
        fs::set_permissions(&service_directory, fs::Permissions::from_mode(0o700)).unwrap();
        let observed_directory = root.path().join("observed project");
        fs::create_dir(&observed_directory).unwrap();
        let peers = root.path().join("peers");
        fs::create_dir(&peers).unwrap();
        if corroborated {
            fs::write(peers.join(format!("{}.json",std::process::id())),serde_json::to_vec(&serde_json::json!({"pid":std::process::id(),"sessionId":SESSION_ID,"name":"Observed busy peer","cwd":observed_directory,"status":"busy","startedAt":1760000000000_i64,"updatedAt":1760000001000_i64,"statusUpdatedAt":1760000001000_i64,"kind":"interactive","entrypoint":"cli","peerProtocol":1,"messagingSocketPath":peers.join("peer.sock")})).unwrap()).unwrap();
        }
        let jobs = root.path().join("jobs.json");
        fs::write(&jobs,serde_json::to_vec(&serde_json::json!([{"kind":"background","id":"separate-background-handle","sessionId":SESSION_ID,"cwd":observed_directory,"name":"Active native background","state":"working","startedAt":1760000000000_i64}])).unwrap()).unwrap();
        let executable = root.path().join("native-list");
        codex_router_test_support::node_fixture::write_node_fixture(
            &executable,
            "native-cli-catalogue-agent.ts",
            &serde_json::json!({"kind":"file","output":{"kind":"path","path":jobs}}),
        )
        .unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        let description=serde_json::from_value(serde_json::json!({"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"claude-local"},"label":"Native Claude","availability":{"state":"unavailable","observedAt":"2026-10-18T00:00:00Z","reason":"ACP unavailable"},"channels":[]})).unwrap();
        let mut identity = ServiceIdentity::new(
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
            CollaborationApplication::new(identity),
        )
        .await
        .unwrap();
        let target = ClaudeLaunchTarget {
            service_directory,
            secret_root: root.path().join("secrets"),
            claude_projects_directory: None,
        };
        let (records, error) = target.session_records(10).await.unwrap();
        assert_eq!(
            error.is_some(),
            !background_available,
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
            serde_json::json!(observed_directory)
        );
        if background_available {
            assert_eq!(records[0]["jobHandle"], "separate-background-handle");
        } else {
            assert!(records[0].get("jobHandle").is_none());
        }
        assert_eq!(
            records[0]["updatedAtUnixSeconds"],
            if corroborated {
                serde_json::json!(1760000001_i64)
            } else {
                serde_json::Value::Null
            }
        );
        assert_eq!(
            target.resume_working_directory(SESSION_ID).await,
            Some(observed_directory)
        );
        api.stop().await.unwrap();
    }
}
