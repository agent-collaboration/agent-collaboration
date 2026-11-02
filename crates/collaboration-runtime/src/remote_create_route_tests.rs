//! Route storage is real; the known target ID stands in for the not-yet-wired create owner.
use super::native_models::NativeModelsFixture;
use super::{LOCAL, SOURCE, admission, session, work};
use crate::{
    NativeControlBackend, PreparedRemoteCreate, PreparedRemoteCreateActors,
    ServiceInteractionBroker,
};
use codex_acp_adapter::{ApprovalBroker as _, ApprovalRoute};
use collaboration_protocol::{ProviderRequestedSettings, RouterAccess, UuidIdentity};

async fn prepared(root: &std::path::Path) -> (PreparedRemoteCreate, NativeControlBackend) {
    let fixture = NativeModelsFixture::start();
    let backend = fixture.backend.clone();
    let mut entry = work(root, true);
    entry.request.settings = Some(ProviderRequestedSettings {
        model: Some("fixture-model".to_owned()),
        effort: Some("low".to_owned()),
        mode: None,
    });
    let mut policy = admission(Some(session(LOCAL)), true);
    policy.inputs.native_backend = Some(backend.clone());
    let prepared = policy
        .prepare_authenticated_work(&entry)
        .await
        .expect("actual target preflight");
    assert_eq!(
        fixture.finish().await,
        ["initialize", "initialized", "model/list"]
    );
    (prepared, backend)
}

fn route(prepared: &PreparedRemoteCreate) -> ApprovalRoute {
    let PreparedRemoteCreateActors::Codex {
        creator, approver, ..
    } = prepared.actors()
    else {
        panic!("native actors required")
    };
    ApprovalRoute {
        source_installation: None,
        thread_id: "known-created-target".to_owned(),
        created_by: creator.clone(),
        approver: approver.clone(),
        access: RouterAccess::WorkspaceWrite,
        scratch_path: "/fixture-scratch".to_owned(),
        root_message_id: None,
    }
}

async fn load(
    root: &std::path::Path,
    backend: NativeControlBackend,
) -> std::sync::Arc<ServiceInteractionBroker> {
    ServiceInteractionBroker::load(
        UuidIdentity::try_from(LOCAL.to_owned()).expect("local ID"),
        backend,
        root.join("approval-routes.json"),
    )
    .await
    .expect("broker load")
}

#[tokio::test]
async fn admitted_foreign_creator_route_reopens_with_exact_provenance() {
    let root = tempfile::tempdir().expect("route root");
    let (prepared, backend) = prepared(root.path()).await;
    let expected = route(&prepared);
    let broker = load(root.path(), backend.clone()).await;
    broker
        .register_remote_create_route(&prepared, expected.clone())
        .await
        .expect("admitted remote route");
    drop(broker);
    let reopened = load(root.path(), backend).await;
    let stored = reopened
        .route(&expected.thread_id)
        .await
        .expect("route read")
        .expect("same known target");
    assert_eq!(stored.created_by, expected.created_by);
    assert_eq!(stored.approver, expected.approver);
    assert_eq!(
        stored
            .source_installation
            .as_ref()
            .map(message_board::ServiceId::as_str),
        Some(SOURCE)
    );
    assert_eq!(stored.scratch_path, expected.scratch_path);
    assert_eq!(stored.access, expected.access);
}

#[tokio::test]
async fn ordinary_registration_never_accepts_supplied_provenance() {
    let root = tempfile::tempdir().expect("route root");
    let (prepared, backend) = prepared(root.path()).await;
    let broker = load(root.path(), backend).await;
    let mut local = route(&prepared);
    local.created_by = session(LOCAL);
    broker
        .register_route(local.clone())
        .await
        .expect("ordinary local route");
    local.source_installation = Some(prepared.entry().source_installation.clone());
    assert!(broker.register_route(local).await.is_err());
    assert!(broker.register_route(route(&prepared)).await.is_err());
}

#[tokio::test]
async fn remote_registration_rejects_route_parties_that_do_not_match_the_plan() {
    let root = tempfile::tempdir().expect("route root");
    let (prepared, backend) = prepared(root.path()).await;
    let broker = load(root.path(), backend).await;
    for change in 0..5 {
        let mut mismatched = route(&prepared);
        match change {
            0 => mismatched.created_by = session(LOCAL),
            1 => mismatched.approver = session(SOURCE),
            2 => {
                mismatched.source_installation = Some(prepared.entry().source_installation.clone())
            }
            3 => mismatched.thread_id.clear(),
            _ => mismatched.access = RouterAccess::WriteRestricted,
        }
        assert!(
            broker
                .register_remote_create_route(&prepared, mismatched)
                .await
                .is_err()
        );
    }
    assert!(!root.path().join("approval-routes.json").exists());
}

#[tokio::test]
async fn stored_routes_fail_closed_on_missing_or_mismatched_foreign_provenance() {
    let root = tempfile::tempdir().expect("preflight root");
    let (prepared, backend) = prepared(root.path()).await;
    for change in 0..3 {
        let storage = tempfile::tempdir().expect("separate route root");
        let mut corrupt = route(&prepared);
        corrupt.source_installation = match change {
            0 => None,
            1 => Some(message_board::ServiceId::try_from(LOCAL.to_owned()).expect("wrong source")),
            _ => {
                corrupt.approver = session(SOURCE);
                Some(prepared.entry().source_installation.clone())
            }
        };
        std::fs::write(
            storage.path().join("approval-routes.json"),
            serde_json::to_vec(&vec![corrupt]).expect("stored route JSON"),
        )
        .expect("write stored row");
        assert!(
            ServiceInteractionBroker::load(
                UuidIdentity::try_from(LOCAL.to_owned()).expect("local ID"),
                backend.clone(),
                storage.path().join("approval-routes.json")
            )
            .await
            .is_err()
        );
    }
}

#[tokio::test]
async fn existing_local_route_bytes_reopen_without_a_provenance_field() {
    let root = tempfile::tempdir().expect("route root");
    let (prepared, backend) = prepared(root.path()).await;
    let mut local = route(&prepared);
    local.created_by = session(LOCAL);
    let bytes = serde_json::to_vec(&vec![local.clone()]).expect("local stored route");
    assert!(
        !String::from_utf8(bytes.clone())
            .expect("JSON UTF8")
            .contains("sourceInstallation")
    );
    std::fs::write(root.path().join("approval-routes.json"), bytes).expect("old local route bytes");
    let broker = load(root.path(), backend).await;
    let reopened = broker
        .route(&local.thread_id)
        .await
        .expect("read")
        .expect("local route");
    assert_eq!(reopened.created_by, local.created_by);
    assert!(reopened.source_installation.is_none());
}
