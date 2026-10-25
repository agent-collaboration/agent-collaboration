//! BC01: literal board records, copied prefixes, replay and owning reopen.
use super::{
    central_pair::{SecondHost, await_view, central, settled_view, work_view},
    fixture::{BoardFixture, accepted, at},
    records_reopen,
};
use codex_router_test_support::FixtureResult;
use serde_json::{Value, json};

pub async fn replication(fixture: &mut BoardFixture, reader: &mut SecondHost) -> FixtureResult {
    fixture.create().await?;
    let thread = fixture.thread(&fixture.project, &fixture.owner).await?;
    const TEXT: &str = "BC01 immutable replicated post, retained after owning reopen";
    let request = fixture.reply(&fixture.project, &thread, TEXT).await?;
    let first = fixture.write("thread_post", request.clone()).await?;
    accepted(&first)?;
    let original = settled_view(fixture.host()?, &fixture.project).await?;
    let copied = await_view(reader.host()?, &fixture.project, at(&original, "/asOf")?).await?;
    assert_eq!(copied.get("source"), Some(&json!({"kind":"readCopy"})));
    assert_eq!(copied.get("workstreams"), original.get("workstreams"));
    let lanes = at(&copied, "/workstreams")?
        .as_array()
        .ok_or("copied lanes absent")?;
    assert!(
        !lanes.is_empty(),
        "an empty matching view is not replication proof"
    );
    assert!(
        lanes
            .iter()
            .any(|lane| lane.get("id") == Some(&json!(fixture.workstream)))
    );
    assert_eq!(copied.get("asOf"), copied.get("replicatedThrough"));
    let feed = reader
        .host()?
        .mcp(
            "feed_read",
            json!({"projectId":fixture.project,
        "reader":{"kind":"anonymous"},"after":{"epoch":1,"position":0},
        "limit":100,"includeRedacted":false}),
        )
        .await?;
    let posts = matching_posts(&feed, at(&request, "/postId")?)?;
    assert_eq!(posts.len(), 1);
    let post = posts.first().ok_or("copied post absent")?;
    assert_eq!(
        post.pointer("/content/record/record/text"),
        Some(&json!(TEXT))
    );
    let installation = fixture
        .installation
        .as_ref()
        .ok_or("writer identity absent")?;
    assert_eq!(
        post.pointer("/origin/installation"),
        Some(&json!(installation.service_id))
    );
    assert_eq!(
        fixture.write("thread_post", request).await?.get("result"),
        first.get("result")
    );
    assert_eq!(
        settled_view(fixture.host()?, &fixture.project).await?,
        original
    );
    let directory = central(fixture)?
        .directory_entry(&fixture.project.clone().try_into()?)
        .await?;
    assert_eq!(directory.pointer("/copySide/kind"), Some(&json!("known")));
    assert_eq!(
        directory.pointer("/leaseSide/value/lease/value/holder"),
        Some(&json!(installation.service_id))
    );
    assert_eq!(
        directory.pointer("/copySide/value/latestRecordAt"),
        post.get("time")
    );
    fixture.stop_host().await?;
    assert_capture_mapping(fixture, &directory, &copied).await?;
    fixture.reopen_host().await?;
    eprintln!("BC01 checking the reopened writer view");
    assert_eq!(
        work_view(fixture.host()?, &fixture.project).await?,
        original
    );
    reader.stop_host().await?;
    reader.reopen_host().await?;
    eprintln!("BC01 checking the reopened reader view");
    let reopened = await_view(reader.host()?, &fixture.project, at(&copied, "/asOf")?).await?;
    let directory = central(fixture)?
        .directory_entry(&fixture.project.clone().try_into()?)
        .await?;
    assert_reopened_copy(&copied, &reopened, &directory)?;
    let reopened_feed = reader
        .host()?
        .mcp(
            "feed_read",
            json!({"projectId":fixture.project,
        "reader":{"kind":"anonymous"},"after":{"epoch":1,"position":0},
        "limit":100,"includeRedacted":false}),
        )
        .await?;
    assert_eq!(reopened_feed, feed);
    Ok(())
}

fn assert_reopened_copy(before: &Value, after: &Value, directory: &Value) -> FixtureResult {
    let mut expected: message_board::WorkView = serde_json::from_value(before.clone())?;
    let actual: message_board::WorkView = serde_json::from_value(after.clone())?;
    let previous = expected
        .writer_observation
        .ok_or("previous lease observation absent")?;
    let current = actual
        .writer_observation
        .ok_or("reopened lease observation absent")?;
    assert!(current.lease_version >= previous.lease_version);
    assert_eq!(
        serde_json::to_value(current)?,
        json!({
        "leaseVersion":at(directory,"/leaseSide/value/leaseVersion")?,
        "asOf":at(directory,"/leaseSide/value/asOf")?})
    );
    // Reconnection can renew the lease independently of the immutable board prefix.
    expected.writer_observation = Some(current);
    assert_eq!(
        serde_json::to_value(actual)?,
        serde_json::to_value(expected)?
    );
    Ok(())
}

pub fn matching_posts<'a>(feed: &'a Value, post: &Value) -> FixtureResult<Vec<&'a Value>> {
    let entries = at(feed, "/value/entries")?
        .as_array()
        .ok_or("feed entries absent")?;
    assert!(
        !entries.is_empty(),
        "empty feed cannot prove copied records"
    );
    assert_eq!(
        feed.pointer("/value/highWater"),
        feed.pointer("/value/coveredThrough")
    );
    Ok(entries
        .iter()
        .filter(|entry| entry.get("record") == Some(&json!({"kind":"post","id":post})))
        .collect())
}

async fn assert_capture_mapping(
    fixture: &BoardFixture,
    directory: &Value,
    copied: &Value,
) -> FixtureResult {
    let rows = records_reopen::stopped_values(fixture,
        "SELECT json_object('epoch',epoch,'captured',captured_position,'feed',last_feed_position) AS value FROM capture_feed_marks ORDER BY epoch,captured_position").await?;
    let rows = rows
        .into_iter()
        .map(|row| serde_json::from_str::<Value>(&row))
        .collect::<Result<Vec<_>, _>>()?;
    let position = at(directory, "/copySide/value/asOf/position")?;
    let matching = rows
        .iter()
        .filter(|row| {
            row.get("epoch") == position.get("epoch")
                && row.get("captured") == position.get("sequence")
        })
        .collect::<Vec<_>>();
    assert_eq!(
        matching.len(),
        1,
        "Directory must name an actual captured transaction"
    );
    assert_eq!(
        matching.first().ok_or("capture mark absent")?.get("feed"),
        copied.pointer("/asOf/position")
    );
    Ok(())
}
