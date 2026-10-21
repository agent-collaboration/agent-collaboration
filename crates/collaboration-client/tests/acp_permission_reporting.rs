//! Setup metadata survives the public client without requiring a model turn.
use collaboration_client::{
    AcpConversation, CollaborationAccess, ConversationClient, ConversationCreateInput,
    ConversationLoadInput,
};
use collaboration_mcp::test_support::ServedCollaborationApi;
use collaboration_service::{CollaborationApplication, ServiceIdentity};
use serde_json::{Value, json};
use std::{os::unix::fs::PermissionsExt, time::Duration};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[tokio::test]
async fn create_and_load_return_permission_differences_from_acp_setup() {
    for creating in [true, false] {
        // Arrange: discovery and ACP sockets are real; only the upstream peer is scripted.
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("service");
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        let service_id = "00000000-0000-4000-8000-000000000001";
        let endpoint = json!({"serviceId":service_id,"endpointId":"codex-local"});
        let actor = json!({"endpoint":endpoint,"sessionId":"creator"});
        let identity = ServiceIdentity::new(service_id, service_id).unwrap().with_endpoints(vec![serde_json::from_value(json!({
            "endpoint":endpoint,"label":"ACP setup fixture","availability":{"state":"available","observedAt":"2026-09-06T00:00:00Z"},
            "channels":[{"kind":"acp","transport":"unixJsonLines","path":"acp.sock","schemaDigest":format!("sha256:{}",collaboration_protocol::ACP_SCHEMA_DIGEST)}]
        })).unwrap()]).unwrap();
        let api = ServedCollaborationApi::start(&root, CollaborationApplication::new(identity))
            .await
            .unwrap();
        let listener = tokio::net::UnixListener::bind(root.join("acp.sock")).unwrap();
        let peer = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (read, mut write) = stream.into_split();
            let mut lines = BufReader::new(read).lines();
            let initialized: Value =
                serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
            assert_eq!(initialized["method"], "initialize");
            write.write_all(format!("{}\n", json!({"jsonrpc":"2.0","id":initialized["id"],"result":{"protocolVersion":1,"agentCapabilities":{"loadSession":true},"authMethods":[]}})).as_bytes()).await.unwrap();
            let request: Value =
                serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
            assert_eq!(
                request["method"],
                if creating {
                    "session/new"
                } else {
                    "session/load"
                }
            );
            let mut result =
                json!({"_meta":{"codexRouter":{"permissionDifferences":["writableRoots"]}}});
            if creating {
                result["sessionId"] = json!("created-thread");
            }
            write
                .write_all(
                    format!(
                        "{}\n",
                        json!({"jsonrpc":"2.0","id":request["id"],"result":result})
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
        });
        let acp = AcpConversation::connect(
            &CollaborationAccess::api(&root),
            "codex-local".to_owned().try_into().unwrap(),
        )
        .await
        .unwrap();
        let client = ConversationClient::CodexAcp(Box::new(acp));
        // Act: no prompt is needed to obtain setup evidence.
        let result = if creating {
            let input: ConversationCreateInput = serde_json::from_value(json!({
                "operationId":"01900000-0000-7000-8000-000000000001","endpoint":endpoint,"workingDirectory":root,
                "access":"workspace-write","createdBy":actor,"approver":actor,"model":"gpt-6-luna","effort":"low"
            })).unwrap();
            serde_json::to_value(client.create(input, Duration::from_secs(2)).await.unwrap())
                .unwrap()
        } else {
            let input: ConversationLoadInput = serde_json::from_value(json!({
                "target":{"endpoint":endpoint,"sessionId":"loaded-thread"},"workingDirectory":root,
                "access":"workspace-write","requestedBy":actor
            }))
            .unwrap();
            serde_json::to_value(client.load(input, Duration::from_secs(2)).await.unwrap()).unwrap()
        };
        // Assert: the public result retains the literal field-class report from decision 772.
        let differences = if creating {
            &result["permissionDifferences"]
        } else {
            &result["settlement"]["detail"]["permissionDifferences"]
        };
        assert_eq!(differences, &json!(["writableRoots"]));
        peer.await.unwrap();
        api.stop().await.unwrap();
    }
}
