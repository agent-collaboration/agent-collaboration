use super::*;
use collaboration_protocol::{
    EndpointAvailability, EndpointDescription, ProviderRequestedPolicy, ProviderWorkingDirectory,
};
use collaboration_runtime::ProviderSessionRecord;
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};

#[tokio::test]
async fn endpoint_free_admission_refuses_when_two_local_catalogs_match_the_same_session() {
    let root = tempfile::tempdir().unwrap();
    let machine_id: MachineId = "019f0000-0000-7000-8000-00000000c301".parse().unwrap();
    let directory = EndpointDirectory::new(machine_id.clone());
    let codex_endpoint = EndpointRef {
        service_id: machine_id.clone(),
        endpoint_id: "codex-local".to_owned().try_into().unwrap(),
    };
    let cursor_endpoint = EndpointRef {
        service_id: machine_id.clone(),
        endpoint_id: "cursor-local".to_owned().try_into().unwrap(),
    };
    for endpoint in [codex_endpoint.clone(), cursor_endpoint.clone()] {
        directory
            .publish(EndpointDescription {
                endpoint,
                label: "fixture endpoint".to_owned().try_into().unwrap(),
                availability: EndpointAvailability::Unavailable {
                    observed_at: "2030-01-01T00:00:00Z".to_owned().try_into().unwrap(),
                    reason: "the catalog remains readable without a provider connection"
                        .to_owned()
                        .try_into()
                        .unwrap(),
                    fix: None,
                },
                channels: Vec::new(),
            })
            .unwrap();
    }
    let store = Arc::new(Mutex::new(
        ProviderOperationStore::open(&root.path().join("provider-operations.sqlite"))
            .await
            .unwrap(),
    ));
    let lookup = CentralRecipientDirectory {
        machine_id: machine_id.clone(),
        endpoints: directory,
        codex_home: root.path().to_owned(),
        provider_store: Some(store.clone()),
        claude_sessions: Arc::new(ClaudeCodeSessionRegistry::new(root.path().join("claude"))),
    };
    let address = SessionRef {
        machine_id,
        session_id: "shared-provider-session".parse().unwrap(),
    };
    assert!(matches!(
        lookup.resolve(&address).await.unwrap(),
        RecipientMatch::Unknown
    ));

    let options = SqliteConnectOptions::new()
        .filename(root.path().join("state_5.sqlite"))
        .create_if_missing(true);
    let mut native_store = SqliteConnection::connect_with(&options).await.unwrap();
    sqlx::query("CREATE TABLE threads (id TEXT PRIMARY KEY)")
        .execute(&mut native_store)
        .await
        .unwrap();
    sqlx::query("INSERT INTO threads (id) VALUES (?)")
        .bind(address.session_id.as_str())
        .execute(&mut native_store)
        .await
        .unwrap();
    native_store.close().await.unwrap();
    assert!(matches!(lookup.resolve(&address).await.unwrap(),
        RecipientMatch::Known(endpoint) if endpoint == codex_endpoint));

    let target = HarnessSessionRef {
        endpoint: cursor_endpoint,
        session_id: address.session_id.as_str().to_owned().try_into().unwrap(),
    };
    store
        .lock()
        .await
        .record_session(&ProviderSessionRecord {
            target: target.clone(),
            working_directory: ProviderWorkingDirectory::try_from(
                root.path().display().to_string(),
            )
            .unwrap(),
            requested_policy: ProviderRequestedPolicy {
                access: collaboration_protocol::RouterAccess::WriteRestricted,
            },
            created_by: target.clone().into(),
            approver: target.into(),
            updated_at_ms: 1,
        })
        .await
        .unwrap();
    assert!(matches!(
        lookup.resolve(&address).await.unwrap(),
        RecipientMatch::Ambiguous
    ));
    assert_eq!(
        lookup.answer(&address).await.unwrap(),
        DeliveryAnswer::Refused {
            reason: MessageRefusal::RecipientRefused,
        }
    );
}
