//! BR12: distinct actual Rust peers receive their own frozen role-notification items.
#![deny(clippy::too_many_lines)]
use super::{
    fixture::{BoardFixture, at, identity},
    notification_peer::NotificationPeer,
    role_checks::Roles,
};
use codex_router_test_support::{
    FixtureResult, owned_cli_process::run_cli_output, scenario_finalization::run_scenario,
};
use collaboration_client::protocol::{EndpointId, EndpointRef, RouterLink, SessionRef};
use serde_json::{Value, json};
use std::{ffi::OsStr, path::Path, time::Duration};

const POST_SUMMARY: &str = "New thread post";

pub async fn two_reviewers(fixture: &mut BoardFixture) -> FixtureResult {
    fixture.create().await?;
    run_scenario(
        Vec::new(),
        async |peers: &mut Vec<NotificationPeer>| {
            let installation = fixture
                .installation
                .as_ref()
                .ok_or("installation identity absent")?;
            let endpoint = EndpointRef {
                service_id: installation.service_id.clone(),
                endpoint_id: EndpointId::try_from("claude-local".to_owned())?,
            };
            for _ in 0..3 {
                peers.push(
                    NotificationPeer::start(
                        &fixture.directory,
                        endpoint.clone(),
                        identity().try_into()?,
                    )
                    .await?,
                );
            }
            assert_eq!(
                peers
                    .iter()
                    .map(|peer| peer.pid)
                    .collect::<std::collections::BTreeSet<_>>()
                    .len(),
                3
            );
            let actors = peers
                .iter()
                .map(|peer| json!({"kind":"session","session":peer.target}))
                .collect::<Vec<_>>();
            assert_eq!(
                actors
                    .iter()
                    .map(Value::to_string)
                    .collect::<std::collections::BTreeSet<_>>()
                    .len(),
                3
            );
            let mut roles = Roles::new();
            for actor in actors.iter().take(2) {
                roles
                    .grant(
                        fixture,
                        &fixture.owner,
                        "workstream",
                        "reviewer",
                        actor,
                        json!("changed"),
                    )
                    .await?;
            }
            let thread = fixture.thread(&fixture.project, &fixture.owner).await?;
            post_and_observe(fixture, peers, &thread, [0, 1]).await?;
            assert_counts(peers, [1, 1, 0]).await?;
            roles
                .change(
                    fixture,
                    &fixture.owner,
                    "workstream",
                    json!({"kind":"roleRelease",
            "role":"reviewer","holder":actors.first().ok_or("first reviewer absent")?}),
                    json!("changed"),
                )
                .await?;
            roles
                .grant(
                    fixture,
                    &fixture.owner,
                    "workstream",
                    "reviewer",
                    actors.get(2).ok_or("replacement reviewer absent")?,
                    json!("changed"),
                )
                .await?;
            post_and_observe(fixture, peers, &thread, [1, 2]).await?;
            assert_counts(peers, [1, 2, 1]).await?;
            // Join the producer before the final peer barriers can certify no later injection.
            fixture.stop_host().await?;
            assert_counts(peers, [1, 2, 1]).await
        },
        shutdown_peers,
        || Ok(()),
    )
    .await
}

async fn shutdown_peers(peers: Vec<NotificationPeer>) -> FixtureResult {
    let mut failures = Vec::new();
    for peer in peers {
        if let Err(error) = peer.shutdown().await {
            failures.push(error.to_string());
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; ").into())
    }
}

async fn assert_counts(peers: &mut [NotificationPeer], expected: [u64; 3]) -> FixtureResult {
    for (peer, expected) in peers.iter_mut().zip(expected) {
        assert_eq!(
            peer.count_matching(POST_SUMMARY).await?,
            expected,
            "post injection count changed for an exact live recipient"
        );
    }
    Ok(())
}

async fn post_and_observe(
    fixture: &BoardFixture,
    peers: &mut [NotificationPeer],
    thread: &str,
    recipients: [usize; 2],
) -> FixtureResult {
    let mut request =
        fixture.reply_request(&fixture.project, thread, "distinct reviewer notification")?;
    request
        .as_object_mut()
        .ok_or("post request absent")?
        .insert(
            "addressees".into(),
            json!([
                {"kind":"role","recipient":{"kind":"reviewer","subject":fixture.workstream}}
            ]),
        );
    let cause = at(&request, "/postId")?.clone();
    fixture.accept("thread_post", request).await?;
    let intents = latest_cause_intents(fixture, &cause).await?;
    assert_eq!(
        intents.len(),
        2,
        "one distinct frozen intent is required for each addressed reviewer"
    );
    let mut delivered_ids = std::collections::BTreeSet::new();
    for slot in recipients {
        let peer = peers.get_mut(slot).ok_or("recipient peer absent")?;
        let target = peer.target.clone();
        let text = peer.expect_text(POST_SUMMARY).await?;
        let reference = injected_reference(&text)?;
        let shown = fixture
            .host()?
            .mcp(
                "router_show",
                json!({"caller":target,"reference":reference}),
            )
            .await?;
        let record = at(&shown, "/record")?;
        assert_eq!(record.get("kind"), Some(&json!("board-notification")));
        assert_eq!(record.get("target"), Some(&serde_json::to_value(&target)?));
        assert_eq!(
            record.pointer("/headerFacts/project"),
            Some(&json!(fixture.project))
        );
        assert_eq!(
            record.pointer("/headerFacts/cause"),
            Some(&json!({"kind":"post","id":cause}))
        );
        let intent = intents
            .iter()
            .find(|intent| {
                intent.pointer("/recipient/recipient/actor/session") == Some(&json!(target))
            })
            .ok_or("no frozen intent matches the actual recipient process")?;
        assert_eq!(
            record.pointer("/headerFacts/intent"),
            intent.get("intentId")
        );
        let push = at(record, "/pushId")?.as_str().ok_or("push ID absent")?;
        assert_eq!(reference.rsplit('/').next(), Some(push));
        assert!(
            delivered_ids.insert(push.to_owned()),
            "two recipients shared one actual item identity"
        );
        assert_cli_matches(fixture, &target, &reference, record).await?;
        let other = peers
            .iter()
            .find(|peer| peer.target != target)
            .ok_or("other recipient absent")?;
        let hidden = fixture
            .host()?
            .mcp_reply(
                "router_show",
                json!({"caller":other.target,"reference":reference}),
            )
            .await?;
        assert!(
            hidden.is_error,
            "a different recipient read another peer's notification"
        );
        assert_eq!(hidden.value.get("kind"), Some(&json!("rejected")));
        assert_eq!(
            hidden.value.get("serviceKind"),
            Some(&json!("notPermitted"))
        );
        assert_eq!(hidden.value.get("effect"), Some(&json!("none")));
    }
    assert_eq!(delivered_ids.len(), 2);
    Ok(())
}

async fn latest_cause_intents(fixture: &BoardFixture, cause: &Value) -> FixtureResult<Vec<Value>> {
    let mut intents = std::collections::BTreeMap::new();
    for intent in fixture.feed_records("notifyIntent").await? {
        if intent.get("cause") == Some(&json!({"kind":"post","id":cause})) {
            let id = at(&intent, "/intentId")?
                .as_str()
                .ok_or("intent identity absent")?
                .to_owned();
            // The feed retains state transitions under the same frozen intent identity.
            intents.insert(id, intent);
        }
    }
    Ok(intents.into_values().collect())
}

fn injected_reference(text: &str) -> FixtureResult<String> {
    let reference = text
        .split_whitespace()
        .find(|part| part.starts_with("router://"))
        .ok_or("actual injected notification omitted its public Router reference")?
        .trim_end_matches(|character: char| !character.is_ascii_alphanumeric() && character != '-');
    RouterLink::parse(reference)?;
    Ok(reference.to_owned())
}

async fn cli_show_for_session(
    fixture: &BoardFixture,
    caller: &SessionRef,
    reference: &str,
) -> FixtureResult<Value> {
    let mut command =
        super::owned_host::artifact_command(Path::new(env!("CARGO_BIN_EXE_agent-collaboration")))?;
    command.args([
        OsStr::new("show"),
        OsStr::new(reference),
        OsStr::new("--service-directory"),
        fixture.directory.as_os_str(),
        OsStr::new("--json"),
    ]);
    command
        .env_remove("CODEX_THREAD_ID")
        .env_remove("CURSOR_CONVERSATION_ID")
        .env(
            "CLAUDE_CODE_SESSION_ID",
            String::from(caller.session_id.clone()),
        );
    let output = run_cli_output(command, Duration::from_secs(20)).await?;
    assert!(
        output.status.success(),
        "recipient CLI show failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let reply: Value = serde_json::from_slice(&output.stdout)?;
    at(&reply, "/result").cloned()
}

async fn assert_cli_matches(
    fixture: &BoardFixture,
    target: &SessionRef,
    reference: &str,
    record: &Value,
) -> FixtureResult {
    let cli = cli_show_for_session(fixture, target, reference).await?;
    let cli = at(&cli, "/record")?;
    for field in [
        "pushId",
        "kind",
        "target",
        "originRouterRef",
        "headerFacts",
        "body",
    ] {
        assert_eq!(
            cli.get(field),
            record.get(field),
            "CLI/MCP notification payload differs at {field}"
        );
    }
    Ok(())
}
