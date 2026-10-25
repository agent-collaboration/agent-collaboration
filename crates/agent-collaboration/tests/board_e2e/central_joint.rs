//! BC05 composes O's real importer publication with actual claim and A/B/A Hosts.
use super::{
    central_forwarding::assert_forwarded_entry,
    central_holders::write_lane,
    central_pair::{SecondHost, await_writer, central, settled_view, work_view},
    fixture::{BoardFixture, accepted, at, fields, header, identity},
    owned_host::BoardHost,
};
use board_sync_protocol::ProjectClaimReply;
use codex_router_test_support::FixtureResult;
use serde_json::{Value, json};
use std::time::Duration;

pub async fn after_publication(
    fixture: &mut BoardFixture,
    second: &mut SecondHost,
    unheld: &str,
) -> FixtureResult {
    let initial = central(fixture)?
        .directory_entry(&fixture.project.clone().try_into()?)
        .await?;
    assert_eq!(
        initial.pointer("/leaseSide/value/lease/kind"),
        Some(&json!("unheld"))
    );
    assert_unheld(fixture, second, unheld).await?;
    claim_and_install(fixture).await?;
    let first = write_lane(fixture, fixture.host()?, second.host()?, 2).await?;
    central(fixture)?
        .move_project(
            fixture.project.clone().try_into()?,
            &second.service_id()?.try_into()?,
            identity().try_into()?,
        )
        .await?;
    let moved = await_writer(second.host()?, &fixture.project, 3).await?;
    assert_eq!(moved.get("workstreams"), first.get("workstreams"));
    assert_nonpost_refused(fixture, second.host()?, fixture.host()?).await?;
    let moved = write_lane(fixture, second.host()?, fixture.host()?, 3).await?;
    let first_installation = fixture
        .installation
        .as_ref()
        .ok_or("first identity absent")?
        .service_id
        .clone();
    central(fixture)?
        .take_over_project(
            fixture.project.clone().try_into()?,
            &String::from(first_installation).try_into()?,
            identity().try_into()?,
        )
        .await?;
    let returned = await_writer(fixture.host()?, &fixture.project, 4).await?;
    assert_eq!(returned.get("workstreams"), moved.get("workstreams"));
    write_lane(fixture, fixture.host()?, second.host()?, 4).await?;
    assert_nonpost_refused(fixture, fixture.host()?, second.host()?).await?;
    forwarded_posts(fixture, second).await?;
    offline_holder(fixture, second).await?;
    fixture.reopen_host().await?;
    await_writer(fixture.host()?, &fixture.project, 4).await?;
    Ok(())
}

async fn claim_and_install(fixture: &BoardFixture) -> FixtureResult {
    let imported = central(fixture)?
        .copy_status(&fixture.project.clone().try_into()?)
        .await?;
    assert_eq!(imported.holder, None);
    assert_eq!(imported.fence.value(), 1);
    let request = json!({"requestId":identity(),"protocolVersion":1,"projectId":fixture.project});
    let reply: ProjectClaimReply = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let reply: ProjectClaimReply = serde_json::from_value(
                fixture
                    .host()?
                    .mcp("project_claim", request.clone())
                    .await?,
            )?;
            if !matches!(reply, ProjectClaimReply::Pending { .. }) {
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(reply);
            }
            tokio::task::yield_now().await;
        }
    })
    .await??;
    let ProjectClaimReply::Claimed { epoch, start, .. } = reply else {
        return Err(format!("imported claim failed: {reply:?}").into());
    };
    assert_eq!(epoch.value(), 2);
    assert_eq!(start, imported.acked);
    // A claimed answer must expose an installed writer immediately, before any polling helper.
    let installed = work_view(fixture.host()?, &fixture.project).await?;
    assert_eq!(installed.get("source"), Some(&json!({"kind":"writer"})));
    assert_eq!(
        installed.get("currentWriter"),
        Some(&json!({"kind":"held","epoch":2}))
    );
    let grant = fixture.request(
        &fixture.owner,
        json!({"kind":"roleGrant",
        "subject":{"kind":"project","subject":fixture.project},"role":"lead","holder":fixture.owner,
        "model":null,"lineage":null,"expectedRevision":0,"ownerGrant":null}),
    );
    accepted(&fixture.host()?.mcp("role_grant", grant).await?)?;
    Ok(())
}

async fn assert_nonpost_refused(
    fixture: &BoardFixture,
    holder: &BoardHost,
    nonholder: &BoardHost,
) -> FixtureResult {
    let before = settled_view(holder, &fixture.project).await?;
    let request = fixture.request(
        &fixture.owner,
        json!({"kind":"projectDescribe",
        "description":"must not forward this non-post write","expectedRevision":0}),
    );
    let reply = nonholder.mcp("project_describe", request).await?;
    assert_eq!(reply.pointer("/result/kind"), Some(&json!("notWriter")));
    assert_eq!(
        settled_view(holder, &fixture.project).await?.get("asOf"),
        before.get("asOf")
    );
    Ok(())
}

fn root_request(fixture: &BoardFixture, project: &str, line: &str) -> FixtureResult<Value> {
    fields(
        header(&fixture.owner),
        json!({"postId":identity(),"projectId":project,
        "about":{"kind":"project","subject":{"line":line}},"text":line,"references":[],
        "addressees":[],"role":null,"watch":false}),
    )
}

async fn assert_unheld(
    fixture: &BoardFixture,
    sender: &SecondHost,
    project: &str,
) -> FixtureResult {
    let request = root_request(fixture, project, "BC05 unheld must refuse")?;
    let answer = sender.host()?.mcp("thread_create", request).await?;
    assert_eq!(answer.pointer("/result/kind"), Some(&json!("notHeld")));
    Ok(())
}

async fn forwarded_posts(fixture: &BoardFixture, sender: &SecondHost) -> FixtureResult {
    let root = root_request(fixture, &fixture.project, "BC05 actual forwarded root")?;
    let first = sender.host()?.mcp("thread_create", root.clone()).await?;
    accepted(&first)?;
    let thread = at(&root, "/postId")?
        .as_str()
        .ok_or("forwarded root ID absent")?;
    let reply = fixture.reply_request(&fixture.project, thread, "BC05 actual forwarded reply")?;
    let posted = sender.host()?.mcp("thread_post", reply.clone()).await?;
    accepted(&posted)?;
    let before = settled_view(fixture.host()?, &fixture.project).await?;
    for (tool, request, original) in [
        ("thread_create", &root, &first),
        ("thread_post", &reply, &posted),
    ] {
        assert_eq!(
            sender
                .host()?
                .mcp(tool, request.clone())
                .await?
                .get("result"),
            original.get("result")
        );
    }
    assert_eq!(
        settled_view(fixture.host()?, &fixture.project)
            .await?
            .get("asOf"),
        before.get("asOf")
    );
    let feed = fixture.host()?.mcp("feed_read", json!({"projectId":fixture.project,
        "reader":{"kind":"anonymous"},"after":{"epoch":4,"position":0},"limit":100,"includeRedacted":false})).await?;
    for request in [&root, &reply] {
        assert_forwarded_entry(&feed, sender, request)?;
    }
    Ok(())
}

async fn offline_holder(fixture: &mut BoardFixture, sender: &SecondHost) -> FixtureResult {
    fixture.stop_host().await?;
    let preview = central(fixture)?
        .takeover_presence(&fixture.project.clone().try_into()?)
        .await?;
    let last_seen = at(&preview, "/lastSeen")?
        .as_i64()
        .ok_or("disconnected holder lastSeen absent")?;
    let text = fixture.root.path().join("offline-post.txt");
    std::fs::write(&text, "BC05 offline holder must not queue this post")?;
    let request = identity();
    let post = identity();
    let arguments = [
        "thread",
        "create",
        "--project-id",
        &fixture.project,
        "--post-id",
        &post,
        "--request-id",
        &request,
        "--about",
        "project:offline holder",
        "--actor",
        &fixture.owner.to_string(),
        "--no-watch",
        "--text-file",
        text.to_str().ok_or("offline text path")?,
    ];
    let arguments = arguments
        .iter()
        .map(std::ffi::OsStr::new)
        .collect::<Vec<_>>();
    let result = sender.host()?.cli(&arguments).await?;
    let reply = at(&result, "/record")?;
    assert_eq!(
        reply.pointer("/result/kind"),
        Some(&json!("holderUnreachable"))
    );
    let actual = chrono::DateTime::parse_from_rfc3339(
        at(reply, "/result/decision/lastSeen")?
            .as_str()
            .ok_or("typed lastSeen absent")?,
    )?;
    assert_eq!(actual.timestamp_millis(), last_seen);
    Ok(())
}
