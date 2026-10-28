//! SQLite checks predicate meaning; the default-stack service journey checks native depth.
use serde_json::{Value, json};
use sqlx::{Connection, SqliteConnection};

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;
type UnreadRows = Vec<(String, String, i64)>;
const ORIGINAL: &str = include_str!("../tests/fixtures/thread_unread_original.sql");
const CURRENT: &str = include_str!("project_thread_unread.sql");

struct FeedCase {
    position: i64,
    kind: String,
    id: String,
    matches: bool,
}

async fn add_case(
    connection: &mut SqliteConnection,
    cases: &mut Vec<FeedCase>,
    kind: &str,
    id: &str,
    request: Option<&str>,
    body: Value,
    matches: bool,
) -> TestResult {
    let position = i64::try_from(cases.len())? + 1;
    let content = json!({"kind":"shown","record":{"kind":kind,"record":body}}).to_string();
    sqlx::query("INSERT INTO project_feed(position,record_kind,record_id,request_id,content) VALUES(?,?,?,?,?)")
        .bind(position).bind(kind).bind(id).bind(request).bind(content)
        .execute(connection).await?;
    cases.push(FeedCase {
        position,
        kind: kind.to_owned(),
        id: id.to_owned(),
        matches,
    });
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn thread_unread_case_dispatch_preserves_every_original_predicate_and_bounded_order()
-> TestResult {
    let mut connection = SqliteConnection::connect("sqlite::memory:").await?;
    sqlx::raw_sql("CREATE TABLE project_feed(position INTEGER PRIMARY KEY,record_kind TEXT,record_id TEXT,request_id TEXT,content TEXT); CREATE INDEX feed_by_request_position ON project_feed(request_id,position); CREATE TABLE thread_refilings(refiling_id TEXT,thread_id TEXT); CREATE TABLE posts(post_id TEXT,thread_id TEXT); CREATE TABLE notify_intents(intent_id TEXT,cause_kind TEXT,cause_id TEXT);")
        .execute(&mut connection).await?;
    sqlx::raw_sql("INSERT INTO thread_refilings VALUES('refile-hit','scope'),('refile-miss','other'); INSERT INTO posts VALUES('reply-hit','scope'),('reply-miss','other'); INSERT INTO notify_intents VALUES('intent-hit','thread','scope'),('intent-kind-miss','task','scope'),('intent-id-miss','thread','other');")
        .execute(&mut connection).await?;
    let mut cases = Vec::new();
    for (kind, id, matches) in [
        ("thread", "scope", true),
        ("thread", "other", false),
        ("threadRefiled", "refile-hit", true),
        ("threadRefiled", "refile-miss", false),
        ("post", "scope", true),
        ("post", "reply-hit", true),
        ("post", "reply-miss", false),
        ("notifyIntent", "intent-hit", true),
        ("notifyIntent", "intent-kind-miss", false),
        ("notifyIntent", "intent-id-miss", false),
        ("task", "scope", false),
    ] {
        add_case(
            &mut connection,
            &mut cases,
            kind,
            id,
            None,
            json!({}),
            matches,
        )
        .await?;
    }
    for (id, body, matches) in [
        (
            "fact-link",
            json!({"links":[{"kind":"thread","id":"scope"}],"kind":{"kind":"ownerWords"}}),
            true,
        ),
        (
            "fact-link-kind-miss",
            json!({"links":[{"kind":"task","id":"scope"}]}),
            false,
        ),
        (
            "fact-link-id-miss",
            json!({"links":[{"kind":"thread","id":"other"}]}),
            false,
        ),
        (
            "fact-evaluator",
            json!({"kind":{"kind":"evaluatorResult","fact":{"subject":{"kind":"thread","id":"scope"}}}}),
            true,
        ),
        (
            "fact-evaluator-kind-miss",
            json!({"kind":{"kind":"evaluatorResult","fact":{"subject":{"kind":"task","id":"scope"}}}}),
            false,
        ),
        (
            "fact-evaluator-id-miss",
            json!({"kind":{"kind":"evaluatorResult","fact":{"subject":{"kind":"thread","id":"other"}}}}),
            false,
        ),
        (
            "fact-resolved",
            json!({"kind":{"kind":"lifecycle","fact":{"kind":"threadResolved","thread":"scope"}}}),
            true,
        ),
        (
            "fact-resolved-id-miss",
            json!({"kind":{"kind":"lifecycle","fact":{"kind":"threadResolved","thread":"other"}}}),
            false,
        ),
        (
            "fact-lifecycle-kind-miss",
            json!({"kind":{"kind":"lifecycle","fact":{"kind":"closed","thread":"scope"}}}),
            false,
        ),
        (
            "fact-kind-miss",
            json!({"kind":{"kind":"ownerWords","fact":{"kind":"threadResolved","thread":"scope"}}}),
            false,
        ),
        (
            "fact-two-reasons",
            json!({"links":[{"kind":"thread","id":"scope"}],"kind":{"kind":"lifecycle","fact":{"kind":"threadResolved","thread":"scope"}}}),
            true,
        ),
    ] {
        add_case(&mut connection, &mut cases, "fact", id, None, body, matches).await?;
    }
    // A request counts only for an accepted decision and a matching returned subject.
    for (id, decision, effect_kind, effect_id, matches) in [
        ("request-hit", "accepted", "thread", "scope", true),
        ("request-rejected", "rejected", "thread", "scope", false),
        ("request-kind-miss", "accepted", "task", "scope", false),
        ("request-id-miss", "accepted", "thread", "other", false),
    ] {
        add_case(
            &mut connection,
            &mut cases,
            effect_kind,
            effect_id,
            Some(id),
            json!({}),
            effect_kind == "thread" && effect_id == "scope",
        )
        .await?;
        add_case(
            &mut connection,
            &mut cases,
            "request",
            id,
            Some(id),
            json!({"decision":{"kind":decision}}),
            matches,
        )
        .await?;
    }
    add_case(
        &mut connection,
        &mut cases,
        "request",
        "request-empty",
        Some("request-empty"),
        json!({"decision":{"kind":"accepted"}}),
        false,
    )
    .await?;
    // More than fifty hits separates the total count from the returned reference prefix.
    for index in 0..60 {
        let id = format!("many-{index}");
        sqlx::query("INSERT INTO posts VALUES(?,'scope')")
            .bind(&id)
            .execute(&mut connection)
            .await?;
        add_case(
            &mut connection,
            &mut cases,
            "post",
            &id,
            None,
            json!({}),
            true,
        )
        .await?;
    }
    for (after, through) in [(0_i64, 1000_i64), (7, 29), (999, 999)] {
        let mut selected: Vec<_> = cases
            .iter()
            .filter(|case| case.matches && case.position > after && case.position <= through)
            .collect();
        let count = i64::try_from(selected.len())?;
        selected.reverse();
        let expected: UnreadRows = selected
            .into_iter()
            .take(50)
            .map(|case| (case.kind.clone(), case.id.clone(), count))
            .collect();
        let original: UnreadRows = sqlx::query_as(ORIGINAL)
            .bind("scope")
            .bind(after)
            .bind(through)
            .fetch_all(&mut connection)
            .await?;
        let current: UnreadRows = sqlx::query_as(CURRENT)
            .bind("scope")
            .bind(after)
            .bind(through)
            .fetch_all(&mut connection)
            .await?;
        assert_eq!(
            original, expected,
            "original predicate at ({after},{through}]"
        );
        assert_eq!(current, original, "CASE predicate at ({after},{through}]");
    }
    connection.close().await?;
    Ok(())
}
