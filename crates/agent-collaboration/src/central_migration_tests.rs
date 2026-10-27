//! Literal server pages and observed requests pin continuation without network I/O.

use super::*;
use serde_json::{Value, json};
const FIRST: &str = "019f0000-0000-7000-8000-000000005881";
const SECOND: &str = "019f0000-0000-7000-8000-000000005882";
const THIRD: &str = "019f0000-0000-7000-8000-000000005883";
fn query() -> CopyPageQuery {
    CopyPageQuery {
        after: None,
        limit: Some(2),
    }
}
fn page(results: Value, next: Value) -> Result<CentralAdminAnswer, CentralAdminError> {
    serde_json::from_value::<CopyMigrationPage>(json!({"results":results,"nextAfter":next}))
        .map(CentralAdminAnswer::Migrate)
        .map_err(|error| CentralAdminError::Unavailable(error.to_string()))
}
fn events(output: &[u8]) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    std::str::from_utf8(output)?
        .lines()
        .map(|line| serde_json::from_str(line).map_err(Into::into))
        .collect()
}
#[tokio::test]
async fn migration_follows_pages_and_prints_every_result_then_counts()
-> Result<(), Box<dyn std::error::Error>> {
    let mut replies = std::collections::VecDeque::from([
        page(
            json!([{"project":FIRST,"result":{"kind":"migrated","corpus":"fleet-sync-receiver","from":null,"to":3}},{"project":SECOND,"result":{"kind":"current","corpus":"fleet-sync-receiver","version":3}}]),
            json!(SECOND),
        ),
        page(
            json!([{"project":THIRD,"result":{"kind":"retired"}}]),
            Value::Null,
        ),
    ]);
    let mut cursors = Vec::new();
    let mut output = Vec::new();
    let code = run(
        query(),
        |query| {
            cursors.push(query.after.map(|id| id.as_str().to_owned()));
            std::future::ready(
                replies
                    .pop_front()
                    .expect("migration fetched an unexpected extra page"),
            )
        },
        &mut output,
        true,
    )
    .await;
    let actual = events(&output)?;
    if code != 0
        || cursors != vec![None, Some(SECOND.into())]
        || actual.len() != 4
        || actual.last()
            != Some(
                &json!({"kind":"summary","value":{"counts":{"migrated":1,"current":1,"failed":0,"retired":1},"completion":{"kind":"completed"}}}),
            )
    {
        return Err(format!(
            "migration did not follow exact pages and report literal totals: {actual:?}"
        )
        .into());
    }
    for (event, project) in actual.iter().take(3).zip([FIRST, SECOND, THIRD]) {
        if event.pointer("/value/project") != Some(&json!(project)) {
            return Err("migration omitted or reordered a per-Copy result".into());
        }
    }
    Ok(())
}
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn migration_stops_once_at_a_page_error_with_the_last_completed_resume_cursor()
-> Result<(), Box<dyn std::error::Error>> {
    let mut calls = 0;
    let mut output = Vec::new();
    let code=run(query(),|query| {
        calls+=1;
        assert!(calls<=2,"migration retried an uncertain page");
        let reply=if calls==1 {page(json!([{"project":FIRST,"result":{"kind":"current","corpus":"fleet-sync-receiver","version":3}}]),json!(FIRST))}
            else if query.after.as_ref().map(ProjectId::as_str)==Some(FIRST) {Err(CentralAdminError::OutcomeUnknown("lost page answer".into()))}
            else {Err(CentralAdminError::Unavailable("wrong cursor".into()))};
        std::future::ready(reply)
    },&mut output,true).await;
    let actual = events(&output)?;
    if code == 0
        || calls != 2
        || actual.len() != 2
        || actual
            .last()
            .and_then(|event| event.pointer("/value/completion/resumeAfter"))
            != Some(&json!(FIRST))
        || actual
            .last()
            .and_then(|event| event.pointer("/value/completion/error/kind"))
            != Some(&json!("outcomeUnknown"))
    {
        return Err(format!(
            "uncertain page was retried or lost its completed cursor: {actual:?}, calls={calls}"
        )
        .into());
    }
    Ok(())
}
#[tokio::test]
async fn migration_copy_failure_keeps_later_pages_and_returns_nonzero_totals()
-> Result<(), Box<dyn std::error::Error>> {
    let mut replies = std::collections::VecDeque::from([
        page(
            json!([{"project":FIRST,"result":{"kind":"failed","corpus":"fleet-sync-receiver","version":null,"reason":"bad history"}}]),
            json!(FIRST),
        ),
        page(
            json!([{"project":SECOND,"result":{"kind":"current","corpus":"fleet-sync-receiver","version":3}}]),
            Value::Null,
        ),
    ]);
    let mut calls = 0;
    let mut output = Vec::new();
    let code = run(
        query(),
        |_| {
            calls += 1;
            std::future::ready(
                replies
                    .pop_front()
                    .expect("migration retried a failed page"),
            )
        },
        &mut output,
        true,
    )
    .await;
    let actual = events(&output)?;
    if code == 0
        || calls != 2
        || actual.len() != 3
        || actual
            .last()
            .and_then(|event| event.pointer("/value/counts"))
            != Some(&json!({"migrated":0,"current":1,"failed":1,"retired":0}))
        || actual
            .last()
            .and_then(|event| event.pointer("/value/completion/kind"))
            != Some(&json!("completed"))
    {
        return Err(
            format!("copy-level failure stopped the sweep or disappeared: {actual:?}").into(),
        );
    }
    Ok(())
}
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn migration_http_failure_stops_without_following_another_page()
-> Result<(), Box<dyn std::error::Error>> {
    let mut calls = 0;
    let mut output = Vec::new();
    let code = run(
        query(),
        |_| {
            calls += 1;
            assert!(calls <= 1, "migration retried an HTTP failure");
            std::future::ready(Ok(CentralAdminAnswer::Failure(
                OwnerOperationFailure::ObjectUnavailable,
            )))
        },
        &mut output,
        true,
    )
    .await;
    let actual = events(&output)?;
    if code == 0
        || calls != 1
        || actual
            .last()
            .and_then(|event| event.pointer("/value/completion/resumeAfter"))
            != Some(&Value::Null)
    {
        return Err("HTTP failure did not stop with its beginning resume point".into());
    }
    Ok(())
}
