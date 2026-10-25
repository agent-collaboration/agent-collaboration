//! BR03 / S-R2-F1: one corrupt membership stays unavailable while its reader's healthy project works.
use super::fixture::{BoardFixture, at, fields, header, human};
use codex_router_test_support::FixtureResult;
use serde_json::{Value, json};
use sqlx_turso::sqlx::{ConnectOptions, Connection};

async fn subscribe(fixture: &BoardFixture, reader: &Value, project: &str) -> FixtureResult {
    fixture
        .accept(
            "thread_subscribe",
            fields(
                header(reader),
                json!({"scope":{"kind":"project","projectId":project},
                "policy":{"mode":"poll","quietSeconds":0,"capSeconds":0,"lifetimeSeconds":86400}}),
            )?,
        )
        .await?;
    Ok(())
}

async fn membership(fixture: &BoardFixture, reader: &Value, project: &str) -> FixtureResult<Value> {
    let feed = fixture
        .host()?
        .mcp(
            "feed_read",
            json!({"projectId":project,"reader":{"kind":"anonymous"},
            "after":{"epoch":1,"position":0},"limit":100,"includeRedacted":false}),
        )
        .await?;
    assert_eq!(
        at(&feed, "/value/highWater")?,
        at(&feed, "/value/coveredThrough")?
    );
    at(&feed, "/value/entries")?
        .as_array()
        .ok_or("membership feed entries")?
        .iter()
        .filter_map(|entry| {
            let record = entry.pointer("/content/record")?;
            (record.get("kind") == Some(&json!("subscriptionMembership")))
                .then(|| record.get("record"))
                .flatten()
        })
        .rfind(|record| record.get("reader") == Some(reader))
        .cloned()
        .ok_or_else(|| "membership absent from actual feed".into())
}

async fn other_authored_root(fixture: &BoardFixture, project: &str) -> FixtureResult<String> {
    let author = human(&format!("membership-author-{project}"));
    fixture
        .accept("role_grant", json!({"requestId":super::fixture::identity(),"protocolVersion":1,
            "actingFor":null,"actor":fixture.owner,"projectId":project,"operation":{"kind":"roleGrant",
            "subject":{"kind":"project","subject":project},"role":"advisor","holder":author,
            "model":null,"lineage":null,"expectedRevision":1,"ownerGrant":null}}))
        .await?;
    fixture.thread(project, &author).await
}

async fn corrupt_policy(fixture: &BoardFixture, project: &str, membership: &str) -> FixtureResult {
    // The path helper rejects a live Host. Close this exact fault writer before reopening its owner.
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(super::records_reopen::database_for(fixture, project)?)
        .create_if_missing(false)
        .connect()
        .await?;
    let changed = sqlx_turso::sqlx::query::<sqlx_turso::Turso>(
        "UPDATE subscription_memberships SET policy=json_set(policy,'$.mode','corrupt-mode') WHERE membership_id=?",
    )
    .bind(membership)
    .execute(&mut connection)
    .await;
    let closed = connection.close().await;
    let changed = super::stopped_faults::finish_stopped_access(
        changed.map_err(Into::into),
        closed.map_err(Into::into),
    )?;
    assert_eq!(changed.rows_affected(), 1);
    Ok(())
}

pub async fn corrupt_sibling(fixture: &mut BoardFixture) -> FixtureResult {
    // Wrong build: one invalid stored membership withdraws the Host or blocks every project for that reader.
    fixture.create().await?;
    let (_, broken) = fixture.foreign_project().await?;
    let reader = fixture.owner.clone();
    for project in [&fixture.project, &broken] {
        subscribe(fixture, &reader, project).await?;
    }
    fixture
        .host()?
        .mcp("thread_subscriptions", json!({"actor":reader}))
        .await?;
    let broken_membership = membership(fixture, &reader, &broken).await?;
    let healthy_thread = other_authored_root(fixture, &fixture.project).await?;
    other_authored_root(fixture, &broken).await?;
    let shown = fixture
        .host()?
        .mcp("post_show", json!({"postId":healthy_thread}))
        .await?;
    let placement: message_board::RecordPlacement =
        serde_json::from_value(at(&shown, "/position")?.clone())?;
    let cursor = placement.project_cursor().ok_or("healthy root cursor")?;
    let expected = json!({"threadId":healthy_thread,"projectId":fixture.project,
        "from":cursor,"through":cursor,"postCount":1});
    let before = fixture.host()?.mcp("installation_show", json!({})).await?;
    fixture.stop_host().await?;
    corrupt_policy(
        fixture,
        &broken,
        at(&broken_membership, "/envelope/id")?
            .as_str()
            .ok_or("membership ID")?,
    )
    .await?;
    fixture.reopen_host().await?;
    let after = fixture.host()?.mcp("installation_show", json!({})).await?;
    assert_eq!(at(&before, "/serviceId")?, at(&after, "/serviceId")?);
    assert_eq!(
        fixture
            .cli(&["endpoints", "list"])
            .await?
            .pointer("/page/records"),
        Some(&json!([]))
    );
    assert_healthy_write_and_unavailable_sibling(fixture, &reader, &broken, &expected).await
}

async fn assert_healthy_write_and_unavailable_sibling(
    fixture: &BoardFixture,
    reader: &Value,
    broken: &str,
    expected: &Value,
) -> FixtureResult {
    fixture.accept("workstream_rename", fixture.request(reader,json!({"kind":"workstreamRename",
        "workstreamId":fixture.workstream,"name":"healthy write after corrupt membership","expectedRevision":0}))).await?;
    let workstream = fixture
        .host()?
        .mcp(
            "workstream_show",
            json!({"projectId":fixture.project,
        "record":{"kind":"workstream","id":fixture.workstream}}),
        )
        .await?;
    assert_eq!(
        workstream.pointer("/record/name"),
        Some(&json!("healthy write after corrupt membership"))
    );
    for project in [fixture.project.as_str(), broken] {
        let card = fixture
            .host()?
            .mcp_reply(
                "view_card",
                json!({"projectId":project,"actor":reader,
            "subject":{"kind":"project"}}),
            )
            .await?;
        if project == broken {
            assert!(card.is_error, "corrupt project card must be refused");
            assert_eq!(card.value.get("mcpResult"), Some(&json!("error")));
            assert_eq!(card.value.get("kind"), Some(&json!("boardUnavailable")));
            assert_eq!(card.value.get("stage"), Some(&json!("storage")));
            assert_eq!(card.value.get("nextAction"), Some(&json!("retryLater")));
        } else {
            assert!(
                !card.is_error,
                "healthy project card refused: {}",
                card.value
            );
            let card: message_board::ContinuationCard = serde_json::from_value(card.value)?;
            assert_eq!(card.subject.kind, message_board::RecordKind::Project);
            assert_eq!(card.subject.id.as_str(), project);
            assert_eq!(
                card.counts.get(&message_board::RecordKind::Workstream),
                Some(&1)
            );
        }
    }
    let refused = fixture.host()?.mcp_reply("feed_read", json!({"projectId":broken,
        "reader":{"kind":"anonymous"},"after":{"epoch":1,"position":0},"limit":100,"includeRedacted":false})).await?;
    assert!(refused.is_error);
    assert_eq!(refused.value.get("mcpResult"), Some(&json!("error")));
    assert_eq!(refused.value.get("stage"), Some(&json!("storage")));
    assert!(matches!(
        refused.value.get("kind").and_then(Value::as_str),
        Some("invalidRecord" | "boardUnavailable")
    ));
    let listed = fixture
        .host()?
        .mcp("thread_subscriptions", json!({"actor":reader}))
        .await?;
    assert_eq!(listed.get("unavailable"), Some(&json!([broken])));
    let subscriptions = at(&listed, "/subscriptions")?
        .as_array()
        .ok_or("subscription rows")?;
    assert_eq!(subscriptions.len(), 1);
    let healthy = subscriptions.first().ok_or("healthy subscription")?;
    assert_eq!(healthy.get("projectId"), Some(&json!(fixture.project)));
    assert_eq!(healthy.get("pendingCount"), Some(&json!(1)));
    super::poll_delivery::consume_once(fixture, reader, expected).await?;
    Ok(())
}
