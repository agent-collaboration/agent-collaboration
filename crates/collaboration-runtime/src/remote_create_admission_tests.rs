use super::RemoteCreateAdmissionError::Refused;
use super::*;
use collaboration_protocol::{
    ConversationOperationFailureKind, EndpointDescription, ProviderOperationEffect,
};
use serde_json::{Value, json};

#[path = "remote_create_native_tests.rs"]
mod native_models;
#[path = "remote_create_route_tests.rs"]
mod routes;

const SOURCE: &str = "019f0000-0000-7000-8000-0000000000a1";
const LOCAL: &str = "019f0000-0000-7000-8000-0000000000b2";

fn session(service: &str) -> SessionRef {
    serde_json::from_value(
        json!({"endpoint":{"serviceId":service,"endpointId":"codex-local"},"sessionId":"chief"}),
    )
    .expect("session fixture")
}

fn work(directory: &std::path::Path, codex: bool) -> SessionCreateEntry {
    serde_json::from_value(json!({
        "workItemId":"019f0000-0000-7000-8000-000000000001",
        "operationId":"019f0000-0000-7000-8000-000000000002",
        "sourceInstallation":SOURCE,
        "request":{
            "endpoint":{"serviceId":LOCAL,"endpointId":if codex {"codex-local"} else {"claude-local"}},
            "workingDirectory":directory,
            "createdBy":session(SOURCE),
            "approver":null,
            "requestedPolicy":{"access":"workspace-write"},
            "settings":{"model":"unadvertised-provider-model","effort":"unadvertised-effort"},
            "firstInput":null
        }
    })).expect("central work fixture")
}

fn admission(default_approver: Option<SessionRef>, codex: bool) -> RemoteCreateAdmission {
    let service_id = UuidIdentity::try_from(LOCAL.to_owned()).expect("local identity");
    let endpoints = EndpointDirectory::new(service_id.clone());
    let channel = if codex {
        json!({"kind":"acp","transport":"unixJsonLines","path":"codex-acp.sock","schemaDigest":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"})
    } else {
        json!({"kind":"externalProvider","transport":"stdioAcp","bindingId":"fixture","bindingGeneration":1,
            "runtime":{"provider":"claudeCode","runtimeName":"fixture","runtimeVersion":"1"},
            "capabilities":[{"name":"create","status":"supported","evidence":"advertised"}]})
    };
    let endpoint: EndpointDescription = serde_json::from_value(json!({
        "endpoint":{"serviceId":LOCAL,"endpointId":if codex {"codex-local"} else {"claude-local"}},
        "label":"fixture",
        "availability":{"state":"available","observedAt":"2026-11-01T06:00:00Z"},
        "channels":[channel]
    }))
    .expect("endpoint fixture");
    endpoints
        .publish(endpoint)
        .expect("publish actual directory");
    RemoteCreateAdmission::new(RemoteCreateAdmissionInputs {
        service_id,
        owner_human_id: Some(
            message_board::HumanId::try_from("owner".to_owned()).expect("owner identity"),
        ),
        remote_create_default_approver: default_approver,
        endpoints,
        native_backend: None,
    })
    .expect("valid local policy")
}

#[tokio::test]
async fn absent_interim_approver_is_refused_before_preparation() {
    let root = tempfile::tempdir().expect("fixture directory");
    assert!(matches!(
        admission(None, false)
            .prepare_authenticated_work(&work(root.path(), false))
            .await,
        Err(Refused(RemoteCreateRefusal::ApproverRequired))
    ));
}

#[tokio::test]
async fn local_chief_is_resolved_without_rewriting_the_frozen_request() {
    let root = tempfile::tempdir().expect("fixture directory");
    let entry = work(root.path(), false);
    let prepared = admission(Some(session(LOCAL)), false)
        .prepare_authenticated_work(&entry)
        .await
        .expect("valid provider preflight");
    assert!(entry.request.approver.is_none());
    assert!(prepared.entry().request.approver.is_none());
    let PreparedRemoteCreateActors::Provider { creator, approver } = prepared.actors() else {
        panic!("provider actors required")
    };
    assert_eq!(creator, &entry.request.created_by);
    assert_eq!(approver, &ProviderIdentity::Session(session(LOCAL)));
}

#[tokio::test]
async fn source_mismatch_cannot_become_creator_provenance() {
    let root = tempfile::tempdir().expect("fixture directory");
    let mut entry = work(root.path(), false);
    entry.request.created_by = session(LOCAL).into();
    assert!(matches!(
        admission(Some(session(LOCAL)), false)
            .prepare_authenticated_work(&entry)
            .await,
        Err(Refused(RemoteCreateRefusal::OriginMismatch))
    ));
}

#[tokio::test]
async fn human_creator_on_codex_is_refused_before_backend_access() {
    let root = tempfile::tempdir().expect("fixture directory");
    let mut entry = work(root.path(), true);
    entry.request.created_by = ProviderIdentity::Human {
        human_id: message_board::HumanId::try_from("owner".to_owned()).expect("human identity"),
    };
    assert!(matches!(
        admission(Some(session(LOCAL)), true)
            .prepare_authenticated_work(&entry)
            .await,
        Err(Refused(RemoteCreateRefusal::CreatorNotSupported))
    ));
}

#[tokio::test]
async fn foreign_session_and_nonowner_human_approvers_are_refused() {
    let root = tempfile::tempdir().expect("fixture directory");
    for approver in [
        ProviderIdentity::Session(session(SOURCE)),
        ProviderIdentity::Human {
            human_id: message_board::HumanId::try_from("not-owner".to_owned())
                .expect("human identity"),
        },
    ] {
        let mut entry = work(root.path(), false);
        entry.request.approver = Some(approver);
        assert!(matches!(
            admission(None, false)
                .prepare_authenticated_work(&entry)
                .await,
            Err(Refused(RemoteCreateRefusal::ApproverNotSupported))
        ));
    }
}

#[tokio::test]
async fn configured_owner_is_a_valid_provider_approver() {
    let root = tempfile::tempdir().expect("fixture directory");
    let mut entry = work(root.path(), false);
    entry.request.approver = Some(ProviderIdentity::Human {
        human_id: message_board::HumanId::try_from("owner".to_owned()).expect("human identity"),
    });
    assert!(
        admission(None, false)
            .prepare_authenticated_work(&entry)
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn remote_human_creator_is_not_confused_with_the_target_owner() {
    let root = tempfile::tempdir().expect("fixture directory");
    let mut entry = work(root.path(), false);
    entry.request.created_by = ProviderIdentity::Human {
        human_id: message_board::HumanId::try_from("source-owner".to_owned())
            .expect("source human identity"),
    };
    let prepared = admission(Some(session(LOCAL)), false)
        .prepare_authenticated_work(&entry)
        .await
        .expect("central-attributed human creator");
    assert_eq!(prepared.entry().source_installation.as_str(), SOURCE);
    assert_eq!(
        prepared.entry().request.created_by,
        entry.request.created_by
    );
}

#[tokio::test]
async fn human_cannot_approve_codex_even_when_it_is_the_configured_owner() {
    let root = tempfile::tempdir().expect("fixture directory");
    let mut entry = work(root.path(), true);
    entry.request.approver = Some(ProviderIdentity::Human {
        human_id: message_board::HumanId::try_from("owner".to_owned()).expect("owner identity"),
    });
    assert!(matches!(
        admission(Some(session(LOCAL)), true)
            .prepare_authenticated_work(&entry)
            .await,
        Err(Refused(RemoteCreateRefusal::ApproverNotSupported))
    ));
}

#[tokio::test]
async fn another_target_service_is_refused_before_backend_access() {
    let root = tempfile::tempdir().expect("fixture directory");
    let mut entry = work(root.path(), false);
    entry.request.endpoint.service_id = UuidIdentity::try_from(SOURCE.to_owned()).expect("service");
    let result = admission(Some(session(LOCAL)), false)
        .prepare_authenticated_work(&entry)
        .await;
    let Err(Refused(RemoteCreateRefusal::Local { failure })) = result else {
        panic!("another service cannot be admitted locally")
    };
    assert_eq!(
        failure.kind,
        ConversationOperationFailureKind::InvalidRequest
    );
    assert_eq!(failure.effect, ProviderOperationEffect::None);
}

#[tokio::test]
async fn missing_or_nondirectory_folders_are_rejected_with_no_effect() {
    let root = tempfile::tempdir().expect("fixture directory");
    let file = root.path().join("file");
    std::fs::write(&file, b"fixture").expect("fixture file");
    for folder in [root.path().join("missing"), file] {
        let result = admission(Some(session(LOCAL)), false)
            .prepare_authenticated_work(&work(&folder, false))
            .await;
        let Err(Refused(RemoteCreateRefusal::Local { failure })) = result else {
            panic!("folder must be refused")
        };
        assert_eq!(
            failure.kind,
            ConversationOperationFailureKind::InvalidRequest
        );
        assert_eq!(failure.effect, ProviderOperationEffect::None);
        assert!(failure.target.is_none());
    }
}

#[test]
fn invalid_access_cannot_enter_the_work_item_type() {
    let root = tempfile::tempdir().expect("fixture directory");
    let mut wire = serde_json::to_value(work(root.path(), false)).expect("work JSON");
    wire["request"]["requestedPolicy"]["access"] = Value::String("unrestricted".to_owned());
    assert!(serde_json::from_value::<SessionCreateEntry>(wire).is_err());
}

#[test]
fn foreign_chief_is_rejected_when_policy_is_loaded() {
    let local = UuidIdentity::try_from(LOCAL.to_owned()).expect("local identity");
    let result = RemoteCreateAdmission::new(RemoteCreateAdmissionInputs {
        service_id: local.clone(),
        owner_human_id: None,
        remote_create_default_approver: Some(session(SOURCE)),
        endpoints: EndpointDirectory::new(local),
        native_backend: None,
    });
    assert!(matches!(
        result,
        Err(RemoteCreateConfigurationError::DefaultApproverNotLocal)
    ));
}
