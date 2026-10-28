//! Public project status follows current roles and native SDK custody.
use super::*;
use collaboration_contracts::ProjectReplicationPosition;
use collaboration_mcp::test_support::ServedCollaborationApi;
use fleet_sync_delivery::OutboxStatusState;
use fleet_sync_protocol::DeliveryStatus;
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use tokio::time::timeout;

fn session(id: &str) -> Value {
    json!({"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":id})
}
fn request(project: &ProjectId, actor: Value, operation: Value) -> Value {
    json!({"projectId":project,"requestId":RequestId::generate(),"protocolVersion":1,
        "actingFor":null,"actor":actor,"operation":operation})
}
fn assert_accepted(reply: &Value) {
    assert_eq!(
        reply.pointer("/result/result/decision/kind"),
        Some(&json!("accepted")),
        "{reply}"
    );
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn project_status_follows_current_lead() -> Result<(), TestError> {
    let directory = tempfile::tempdir()?;
    let fixture = Box::pin(ProjectNotificationFixture::open(directory.path())).await?;
    let api_root = directory.path().join("public-project-api");
    tokio::fs::create_dir_all(&api_root).await?;
    std::fs::set_permissions(&api_root, std::fs::Permissions::from_mode(0o700))?;
    let api = ServedCollaborationApi::start(&api_root, fixture.application.clone())?;
    let writer = fixture.harness.projects.ensure(&fixture.project).await?;
    let mut snapshot = writer.store.snapshot().await?;
    let project_record = snapshot
        .record(&fixture.project.clone().into())
        .await?
        .ok_or("project")?;
    let owner = project_record
        .envelope()
        .actor
        .declared_identity()
        .ok_or("reported owner")?;
    snapshot.finish().await?;
    let lead = session("first-project-lead");
    let lead_actor = json!({"kind":"session","session":lead});
    let first_grant = OwnerGrantId::generate();
    assert_accepted(&api.call("owner_grant_record", request(&fixture.project, json!(owner), json!({
        "kind":"ownerGrantRecord","grantId":first_grant,"grant":{"kind":"replaceLead",
            "project":fixture.project,"newHolder":lead_actor,"reason":"explicit fixture owner appoints session lead"},
    }))).await?);
    assert_accepted(
        &api.call(
            "role_grant",
            request(
                &fixture.project,
                json!(owner),
                json!({
                    "kind":"roleGrant","subject":{"kind":"project","subject":fixture.project},
                    "role":"lead","holder":lead_actor,"model":null,"lineage":null,
                    "expectedRevision":0,"ownerGrant":first_grant,
                }),
            ),
        )
        .await?,
    );
    // The seeded frozen handoff was deliberately unacknowledged. Actual native
    // replication makes this original eligible for the real publisher.
    timeout(BOUND, async {
        loop {
            writer
                .sender
                .replicate_once(fixture.harness.central.receiver)
                .await?;
            if fixture
                .harness
                .projects
                .acknowledged_mail_changes(&fixture.project)
                .await?
                .max_handoff_seq
                .is_some()
            {
                return Ok::<_, TestError>(());
            }
        }
    })
    .await??;
    fixture
        .publisher
        .queue_project(fixture.project.clone())
        .await?;
    timeout(BOUND, async {
        loop {
            if matches!(
                fixture
                    .harness
                    .mail
                    .outbox_status(fixture.seed_item)
                    .await?
                    .state,
                OutboxStatusState::Known {
                    status: DeliveryStatus::Queued,
                    ..
                }
            ) {
                return Ok::<_, TestError>(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(|_| io::Error::other("real project publisher did not admit queued SDK custody"))??;
    let item = fixture.seed_item.as_uuid().to_string();
    for caller in [lead.clone(), json!({"kind":"humanUser"})] {
        let shown = api
            .call("router_show", json!({"caller":caller,"reference":item}))
            .await?;
        assert_eq!(
            shown.pointer("/result/mail/status/kind"),
            Some(&json!("queued")),
            "{shown}"
        );
        assert_eq!(
            shown.pointer("/result/mail/context/project"),
            Some(&json!(fixture.project))
        );
    }
    let unrelated = session("not-a-project-lead");
    let hidden = api
        .call("router_show", json!({"caller":unrelated,"reference":item}))
        .await?;
    assert_eq!(
        hidden.pointer("/error/data/kind"),
        Some(&json!("notPermitted")),
        "{hidden}"
    );
    assert_accepted(
        &api.call(
            "role_release",
            request(
                &fixture.project,
                lead_actor.clone(),
                json!({
                    "kind":"roleRelease","subject":{"kind":"project","subject":fixture.project},
                    "role":"lead","expectedRevision":1,
                }),
            ),
        )
        .await?,
    );
    let next = session("current-project-lead");
    let next_actor = json!({"kind":"session","session":next});
    let owner_grant = OwnerGrantId::generate();
    assert_accepted(&api.call("owner_grant_record", request(&fixture.project, json!(owner), json!({
        "kind":"ownerGrantRecord","grantId":owner_grant,"grant":{"kind":"replaceLead",
            "project":fixture.project,"newHolder":next_actor,"reason":"explicit current lead change"},
    }))).await?);
    assert_accepted(
        &api.call(
            "role_grant",
            request(
                &fixture.project,
                json!(owner),
                json!({
                    "kind":"roleGrant","subject":{"kind":"project","subject":fixture.project},
                    "role":"lead","holder":next_actor,"model":null,"lineage":null,
                    "expectedRevision":2,"ownerGrant":owner_grant,
                }),
            ),
        )
        .await?,
    );
    let old = api
        .call("router_show", json!({"caller":lead,"reference":item}))
        .await?;
    assert_eq!(
        old.pointer("/error/data/kind"),
        Some(&json!("notPermitted")),
        "{old}"
    );
    let current = api
        .call("router_show", json!({"caller":next,"reference":item}))
        .await?;
    assert_eq!(
        current.pointer("/result/mail/status/kind"),
        Some(&json!("queued")),
        "{current}"
    );
    drop(writer);
    api.stop().await?;
    fixture.close().await
}
