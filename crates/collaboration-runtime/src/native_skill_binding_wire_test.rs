//! Real native socket binding concurrency and same-connection skill invalidation.
use super::test_support::ScriptedSessionBackend;
use super::*;
use session_event_model::{ProviderSkill, ProviderSkillScope, SkillInvocationName, SkillView};
use std::{os::unix::fs::PermissionsExt, time::Duration};
use tokio_tungstenite::{WebSocketStream, client_async};

async fn request(
    client: &mut WebSocketStream<UnixStream>,
    id: u64,
    method: &str,
    params: Value,
) -> Result<(), Box<dyn std::error::Error>> {
    client
        .send(Message::Text(
            json!({"id":id,"method":method,"params":params})
                .to_string()
                .into(),
        ))
        .await?;
    Ok(())
}
async fn response(
    client: &mut WebSocketStream<UnixStream>,
    id: u64,
) -> Result<Value, Box<dyn std::error::Error>> {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let frame = client.next().await.ok_or("missing native response")??;
            let value: Value = serde_json::from_str(frame.to_text()?)?;
            if value.get("id").is_some() {
                if value.get("id").and_then(Value::as_u64) != Some(id) {
                    return Err("wrong correlated response".into());
                }
                return Ok(value);
            }
        }
    })
    .await?
}
fn view(name: &str) -> Result<SkillView, session_event_model::InvalidProviderSkill> {
    Ok(SkillView::Available {
        skills: vec![ProviderSkill {
            invocation_name: SkillInvocationName::new(name)?,
            description: name.into(),
            scope: ProviderSkillScope::System,
        }],
        errors: vec![],
    })
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on incorrect wire behavior; fixture errors propagate."
)]
async fn held_first_start_cannot_overwrite_overlapping_same_cwd_and_read_does_not_rebind()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))?;
    let backend = ScriptedSessionBackend::new()?;
    let first = backend.session.clone();
    let mut second = first.clone();
    second.session_id = message_board::SessionId::try_from("second-session".to_owned())?;
    backend
        .create_sessions
        .lock()
        .expect("sessions")
        .extend([first.clone(), second.clone()]);
    let held = Arc::new(tokio::sync::Notify::new());
    backend
        .create_gates
        .lock()
        .expect("gates")
        .extend([Some(held.clone()), None]);
    let (first_view, _) = tokio::sync::watch::channel(view("first")?);
    let (second_view, _) = tokio::sync::watch::channel(view("second")?);
    backend.skill_views.lock().expect("views").extend([
        (first.clone(), first_view.clone()),
        (second.clone(), second_view),
    ]);
    let models = vec![ProviderModelEntry::try_new(
        "owned".into(),
        "Owned".into(),
        String::new(),
    )?];
    let context = Arc::new(RouterSessionAppServerContext::new(
        backend.endpoint.clone(),
        ScriptedSessionBackend::actor()?,
        backend.clone(),
        backend.clone(),
        super::test_support::model_catalog(tokio::sync::watch::channel(models).1),
    ));
    let path = root.path().join("native.sock");
    let listener = RouterSessionAppServerListener::bind(&path, context)?;
    let shutdown = CancellationToken::new();
    let serving = tokio::spawn(listener.run(shutdown.clone()));
    let (mut client, _) =
        client_async("ws://localhost/rpc", UnixStream::connect(&path).await?).await?;
    request(&mut client, 1, "thread/start", json!({"cwd":root.path()})).await?;
    tokio::time::timeout(Duration::from_secs(2), backend.created.notified()).await?;
    request(&mut client, 2, "thread/start", json!({"cwd":root.path()})).await?;
    let later = response(&mut client, 2).await?;
    assert!(later.get("result").is_some());
    request(&mut client, 3, "skills/list", json!({"cwds":[root.path()]})).await?;
    assert_eq!(
        response(&mut client, 3).await?["result"]["data"][0]["skills"][0]["name"],
        "second"
    );
    held.notify_one();
    let earlier = response(&mut client, 1).await?;
    request(&mut client, 4, "skills/list", json!({"cwds":[root.path()]})).await?;
    let ambiguous = response(&mut client, 4).await?;
    assert_eq!(ambiguous["result"]["data"][0]["skills"], json!([]));
    assert_eq!(
        ambiguous["result"]["data"][0]["errors"]
            .as_array()
            .ok_or("errors")?
            .len(),
        1
    );
    request(&mut client, 40, "model/list", json!({})).await?;
    assert_eq!(
        response(&mut client, 40).await?["result"]["data"][0]["id"],
        "provider-default"
    );
    request(
        &mut client,
        5,
        "thread/resume",
        json!({"threadId":earlier["result"]["thread"]["id"],"excludeTurns":true}),
    )
    .await?;
    response(&mut client, 5).await?;
    request(
        &mut client,
        6,
        "thread/read",
        json!({"threadId":later["result"]["thread"]["id"],"includeTurns":false}),
    )
    .await?;
    response(&mut client, 6).await?;
    request(&mut client, 41, "model/list", json!({})).await?;
    assert_eq!(
        response(&mut client, 41).await?["result"]["data"][0]["id"],
        "owned"
    );
    request(&mut client, 7, "skills/list", json!({"cwds":[root.path()]})).await?;
    assert_eq!(
        response(&mut client, 7).await?["result"]["data"][0]["skills"][0]["name"],
        "first"
    );
    let (mut other, _) =
        client_async("ws://localhost/rpc", UnixStream::connect(&path).await?).await?;
    request(&mut other, 8, "skills/list", json!({"cwds":[root.path()]})).await?;
    assert_eq!(
        response(&mut other, 8).await?["result"]["data"][0]["skills"],
        json!([])
    );
    // Consume the known binding invalidation before causing a new semantic update.
    request(
        &mut client,
        9,
        "skills/list",
        json!({"cwds":[root.path()],"forceReload":true}),
    )
    .await?;
    response(&mut client, 9).await?;
    first_view.send_replace(view("changed")?);
    let changed = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let frame = client.next().await.ok_or("missing invalidation")??;
            let value: Value = serde_json::from_str(frame.to_text()?)?;
            if value.get("method").and_then(Value::as_str) == Some("skills/changed") {
                return Ok::<_, Box<dyn std::error::Error>>(value);
            }
        }
    })
    .await??;
    assert_eq!(changed["params"], json!({}));
    request(
        &mut client,
        10,
        "skills/list",
        json!({"cwds":[root.path()]}),
    )
    .await?;
    assert_eq!(
        response(&mut client, 10).await?["result"]["data"][0]["skills"][0]["name"],
        "changed"
    );
    client.close(None).await?;
    other.close(None).await?;
    shutdown.cancel();
    serving.await??;
    Ok(())
}
