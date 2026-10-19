//! The real API composes jobs, peers and managed storage without identity conversion.
use collaboration_protocol::{
    NativeClaudeBackgroundState, NativeClaudeInventoryPart, NativeInventoryUnavailableReason,
    NativeSessionScope, NativeSessionSource, NativeSessionView, ProviderSessionListParams,
    ProviderSessionListResult, ProviderSessionSummary,
};
use serde_json::json;

#[path = "support/native_catalogue_fixture.rs"]
mod native_catalogue_fixture;
#[path = "support/served_api.rs"]
mod served_api;
use native_catalogue_fixture::{CatalogueFixture, SESSION, job};

fn query(
    fixture: &CatalogueFixture,
    view: NativeSessionView,
    source: NativeSessionSource,
) -> ProviderSessionListParams {
    ProviderSessionListParams {
        endpoint: fixture.target.endpoint.clone(),
        view,
        scope: NativeSessionScope::Any,
        source,
        query: None,
        page_size: 100,
        cursor: None,
    }
}
fn count_background(page: &ProviderSessionListResult) -> usize {
    page.sessions
        .iter()
        .filter(|summary| matches!(summary, ProviderSessionSummary::ClaudeCodeBackground { .. }))
        .count()
}

#[tokio::test]
async fn native_job_before_first_prompt_and_terminal_history_reach_public_api_with_correct_views() {
    let fixture = CatalogueFixture::start(true).await.unwrap();
    fixture
        .write_jobs(vec![
            job("job-before-prompt", SESSION, "blocked"),
            job(
                "stopped-job",
                "00000000-0000-4000-8000-000000000043",
                "stopped",
            ),
            job(
                "working-job",
                "00000000-0000-4000-8000-000000000044",
                "working",
            ),
            job(
                "unknown-job",
                "00000000-0000-4000-8000-000000000045",
                "future-state",
            ),
        ])
        .unwrap();
    let stored = fixture
        .client
        .list_provider_sessions(query(
            &fixture,
            NativeSessionView::Stored,
            NativeSessionSource::Interactive,
        ))
        .await
        .unwrap();
    assert_eq!(count_background(&stored), 4);
    let coverage = stored.native_coverage.as_ref().unwrap();
    assert_eq!(coverage.peer, NativeClaudeInventoryPart::NotRequested);
    assert_eq!(coverage.background, NativeClaudeInventoryPart::Complete);
    let loaded = fixture
        .client
        .list_provider_sessions(query(
            &fixture,
            NativeSessionView::Loaded,
            NativeSessionSource::Interactive,
        ))
        .await
        .unwrap();
    assert_eq!(count_background(&loaded), 2);
    let blocked = loaded
        .sessions
        .iter()
        .find(|summary| summary.target().session_id == fixture.target.session_id)
        .unwrap();
    assert!(
        matches!(blocked,ProviderSessionSummary::ClaudeCodeBackground {job_handle,state:NativeClaudeBackgroundState::Blocked,peer:None,..} if String::from(job_handle.clone())=="job-before-prompt")
    );
    let wire = serde_json::to_value(blocked).unwrap();
    for invented in ["pid", "messagingSocketPath", "updatedAt", "requestedPolicy"] {
        assert!(wire.get(invented).is_none());
    }
    let active = fixture
        .client
        .list_provider_sessions(query(
            &fixture,
            NativeSessionView::Active,
            NativeSessionSource::Interactive,
        ))
        .await
        .unwrap();
    assert_eq!(active.sessions.len(), 1);
    assert_eq!(
        String::from(active.sessions.first().unwrap().target().session_id.clone()),
        "00000000-0000-4000-8000-000000000044"
    );
    let managed = fixture
        .client
        .list_provider_sessions(query(
            &fixture,
            NativeSessionView::Stored,
            NativeSessionSource::All,
        ))
        .await
        .unwrap();
    assert_eq!(managed.sessions.len(), 1);
    assert!(managed.native_coverage.is_none());
    let wire = serde_json::to_value(managed.sessions.first().unwrap()).unwrap();
    assert_eq!(wire["origin"], "hostedProvider");
    assert_eq!(wire["requestedPolicy"]["access"], "write-restricted");
    assert_eq!(wire["approver"]["humanId"], "saved-managed-approver");
    fixture.api.stop().await.unwrap();
}

#[tokio::test]
async fn native_peer_job_corroboration_and_equal_managed_identity_survive_page_boundaries() {
    let fixture = CatalogueFixture::start(true).await.unwrap();
    fixture
        .write_jobs(vec![job("separate-handle", SESSION, "blocked")])
        .unwrap();
    fixture
        .write_peer(SESSION, "/source/native", "idle")
        .unwrap();
    let mut request = query(
        &fixture,
        NativeSessionView::Loaded,
        NativeSessionSource::All,
    );
    request.query = Some(SESSION.to_owned());
    request.page_size = 1;
    let first = fixture
        .client
        .list_provider_sessions(request.clone())
        .await
        .unwrap();
    assert_eq!(first.sessions.len(), 1);
    assert!(matches!(
        first.sessions.first().unwrap(),
        ProviderSessionSummary::HostedProvider { .. }
    ));
    request.cursor = first.next_cursor.clone();
    assert!(request.cursor.is_some());
    let second = fixture
        .client
        .list_provider_sessions(request.clone())
        .await
        .unwrap();
    assert_eq!(second.sessions.len(), 1);
    assert!(second.next_cursor.is_none());
    assert!(
        matches!(second.sessions.first().unwrap(),ProviderSessionSummary::ClaudeCodeBackground {peer:Some(peer),..} if peer.status==collaboration_protocol::ClaudeCodeInteractiveStatus::Idle)
    );
    assert_eq!(
        first.sessions.first().unwrap().target(),
        second.sessions.first().unwrap().target()
    );
    let mut wrong_view = request.clone();
    wrong_view.view = NativeSessionView::Active;
    assert!(
        fixture
            .client
            .list_provider_sessions(wrong_view)
            .await
            .is_err()
    );
    let foreign: collaboration_protocol::EndpointRef = serde_json::from_value(
        json!({"serviceId":"00000000-0000-4000-8000-000000000099","endpointId":"claude-local"}),
    )
    .unwrap();
    let mut wrong_source = request;
    wrong_source.endpoint = foreign;
    assert!(
        fixture
            .client
            .list_provider_sessions(wrong_source)
            .await
            .is_err()
    );
    fixture
        .write_peer(SESSION, "/conflicting/native-directory", "idle")
        .unwrap();
    assert!(
        fixture
            .client
            .list_provider_sessions(query(
                &fixture,
                NativeSessionView::Loaded,
                NativeSessionSource::Interactive
            ))
            .await
            .is_err(),
        "same UUID with conflicting directories is invalid, never index-deduplicated"
    );
    fixture.api.stop().await.unwrap();
}

#[tokio::test]
async fn native_peer_and_job_unavailability_empty_and_invalid_results_stay_distinct() {
    let fixture = CatalogueFixture::start(false).await.unwrap();
    fixture
        .write_jobs(vec![job("known-job", SESSION, "blocked")])
        .unwrap();
    let page = fixture
        .client
        .list_provider_sessions(query(
            &fixture,
            NativeSessionView::Loaded,
            NativeSessionSource::Interactive,
        ))
        .await
        .unwrap();
    assert_eq!(count_background(&page), 1);
    assert_eq!(
        page.native_coverage.unwrap().peer,
        NativeClaudeInventoryPart::Unavailable {
            reason: NativeInventoryUnavailableReason::ReadUnavailable
        }
    );
    fixture.api.stop().await.unwrap();

    let fixture = CatalogueFixture::start(true).await.unwrap();
    fixture
        .write_peer(SESSION, "/source/native", "idle")
        .unwrap();
    std::fs::remove_file(&fixture.executable).unwrap();
    let page = fixture
        .client
        .list_provider_sessions(query(
            &fixture,
            NativeSessionView::Loaded,
            NativeSessionSource::All,
        ))
        .await
        .unwrap();
    assert_eq!(page.sessions.len(), 2);
    assert!(
        page.sessions
            .iter()
            .any(|summary| matches!(summary, ProviderSessionSummary::HostedProvider { .. }))
    );
    assert!(page.sessions.iter().any(|summary| matches!(
        summary,
        ProviderSessionSummary::ClaudeCodeInteractive { .. }
    )));
    assert_eq!(
        page.native_coverage.unwrap().background,
        NativeClaudeInventoryPart::Unavailable {
            reason: NativeInventoryUnavailableReason::ExecutableUnavailable
        }
    );
    fixture.api.stop().await.unwrap();

    let fixture = CatalogueFixture::start(true).await.unwrap();
    let page = fixture
        .client
        .list_provider_sessions(query(
            &fixture,
            NativeSessionView::Loaded,
            NativeSessionSource::Interactive,
        ))
        .await
        .unwrap();
    assert!(page.sessions.is_empty());
    let coverage = page.native_coverage.unwrap();
    assert_eq!(coverage.peer, NativeClaudeInventoryPart::Complete);
    assert_eq!(coverage.background, NativeClaudeInventoryPart::Complete);
    let mut unidentified = job("unidentified", SESSION, "blocked");
    unidentified.as_object_mut().unwrap().remove("sessionId");
    fixture.write_jobs(vec![unidentified]).unwrap();
    let page = fixture
        .client
        .list_provider_sessions(query(
            &fixture,
            NativeSessionView::Stored,
            NativeSessionSource::Interactive,
        ))
        .await
        .unwrap();
    assert!(page.sessions.is_empty());
    assert_eq!(
        page.native_coverage.unwrap().background,
        NativeClaudeInventoryPart::Incomplete {
            unidentified_jobs: 1
        }
    );
    std::fs::write(&fixture.output, "{PRIVATE_INVALID_VENDOR_OUTPUT").unwrap();
    let error = fixture
        .client
        .list_provider_sessions(query(
            &fixture,
            NativeSessionView::Loaded,
            NativeSessionSource::Interactive,
        ))
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        collaboration_client::ClientError::Rejected { code: -32602, .. }
    ));
    assert!(!format!("{error:?}").contains("PRIVATE_INVALID_VENDOR_OUTPUT"));
    fixture.api.stop().await.unwrap();
}
