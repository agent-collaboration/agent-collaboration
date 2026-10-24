//! Public project status and requested controls use current roles and native SDK custody.
use super::*;
use collaboration_mcp::test_support::ServedCollaborationApi;
use collaboration_service::ProjectReplicationPosition;
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
async fn project_status_and_public_cancel_follow_current_lead_and_keep_first_requester_without_project_writes()
-> Result<(), TestError> {
    let directory = tempfile::tempdir()?;
    let fixture = ProjectNotificationFixture::open(directory.path()).await?;
    let api_root = directory.path().join("public-project-api");
    tokio::fs::create_dir_all(&api_root).await?;
    std::fs::set_permissions(&api_root, std::fs::Permissions::from_mode(0o700))?;
    let api = ServedCollaborationApi::start(&api_root, fixture.application.clone()).await?;
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
    let mut snapshot = writer.store.snapshot().await?;
    let before = (
        snapshot.high_water(),
        snapshot.record_counts().await?,
        snapshot.mail_handoff_head().await?,
    );
    snapshot.finish().await?;
    let rejected_id = collaboration_protocol::MailItemId::generate();
    let denied = api
        .call(
            "project_notification_cancel",
            request(
                &fixture.project,
                json!({"kind":"session","session":unrelated}),
                json!({
                    "kind":"notificationCancel","cancelId":rejected_id,"target":item,
                }),
            ),
        )
        .await?;
    assert_eq!(
        denied.pointer("/error/data/kind"),
        Some(&json!("roleRequired")),
        "{denied}"
    );
    assert!(
        fixture
            .harness
            .mail
            .owner
            .local_cancel_delivery(&rejected_id.as_str().to_owned().try_into()?)
            .await?
            .is_none()
    );
    let cancel = collaboration_protocol::MailItemId::generate();
    let command = || {
        request(
            &fixture.project,
            lead_actor.clone(),
            json!({
                "kind":"notificationCancel","cancelId":cancel,"target":item,
            }),
        )
    };
    let first = api.call("project_notification_cancel", command()).await?;
    assert_eq!(
        first.pointer("/result/outcome/kind"),
        Some(&json!("prevented")),
        "{first}"
    );
    let replay = api.call("project_notification_cancel", command()).await?;
    assert_eq!(
        first, replay,
        "a new project request ID must not change the canonical SDK control"
    );
    let stored = fixture
        .harness
        .mail
        .owner
        .local_cancel_delivery(&cancel.as_str().to_owned().try_into()?)
        .await?
        .ok_or("requester attribution")?;
    let by: Identity = serde_json::from_value(lead_actor.clone())?;
    assert_eq!(
        stored.attribution,
        collaboration_mail_protocol::CancelCause::ByRequester { by }
    );
    let mut snapshot = writer.store.snapshot().await?;
    let after = (
        snapshot.high_water(),
        snapshot.record_counts().await?,
        snapshot.mail_handoff_head().await?,
    );
    assert_eq!(
        after, before,
        "requested control appended a project record, decision or handoff"
    );
    let intents = snapshot.records(RecordKind::NotifyIntent).await?;
    assert!(intents.iter().any(|record| matches!(record, Record::NotifyIntent(intent)
        if matches!(intent.state(), IntentState::HandedOff { item: original } if original.as_str()==item))),
        "manual control rewrote the terminal HandedOff intent");
    snapshot.finish().await?;
    assert!(
        fixture
            .harness
            .mail
            .owner
            .get_push_record(&item.clone().try_into()?)
            .await?
            .is_none()
    );
    assert!(matches!(
        fixture
            .harness
            .mail
            .outbox_status(fixture.seed_item)
            .await?
            .state,
        OutboxStatusState::Known {
            status: DeliveryStatus::Cancelled,
            ..
        }
    ));

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
        Some(&json!("cancelled")),
        "{current}"
    );
    let another = collaboration_protocol::MailItemId::generate();
    let canonical = api
        .call(
            "project_notification_cancel",
            request(
                &fixture.project,
                next_actor,
                json!({"kind":"notificationCancel","cancelId":another,"target":item}),
            ),
        )
        .await?;
    assert_eq!(
        canonical, first,
        "the new lead replaced the SDK's first control identity"
    );
    assert!(
        fixture
            .harness
            .mail
            .owner
            .local_cancel_delivery(&another.as_str().to_owned().try_into()?)
            .await?
            .is_none()
    );
    let canonical_stored = fixture
        .harness
        .mail
        .owner
        .local_cancel_delivery(&cancel.as_str().to_owned().try_into()?)
        .await?
        .ok_or("saved requester")?;
    assert_eq!(canonical_stored.target_item, stored.target_item);
    assert_eq!(canonical_stored.attribution, stored.attribution);
    assert_eq!(canonical_stored.outcome, stored.outcome);
    assert_eq!(canonical_stored.recorded_at, stored.recorded_at);
    drop(writer);
    api.stop().await?;
    fixture.close().await
}
