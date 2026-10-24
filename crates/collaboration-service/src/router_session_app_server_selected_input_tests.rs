//! Facade selection admission; Host preparation is an explicit contract stand-in.
use super::test_support::ScriptedSessionBackend;
use super::*;
use crate::{SelectedItem, SelectedPromptPreparation};

async fn start(backend: &Arc<ScriptedSessionBackend>) -> Result<Value, ThreadMethodError> {
    handle_app_server_thread_request(
        "thread/start",
        json!({"cwd":"/tmp"}),
        Arc::clone(backend) as Arc<dyn SessionCommandPort>,
        Arc::clone(backend) as Arc<dyn SessionEventHub>,
        backend.endpoint.clone(),
        ScriptedSessionBackend::actor().map_err(|_| ThreadMethodError::Unavailable)?,
        &[],
    )
    .await
}

#[tokio::test]
async fn native_capabilities_selected_rejection_has_no_setting_prompt_or_steer_effect()
-> Result<(), Box<dyn std::error::Error>> {
    let skill = json!({"type":"skill","name":"skill","path":"/tmp/native/SKILL.md"});
    let text = json!({"type":"text","text":"$skill argument"});
    let cases = [
        ("turn/start", json!([text, skill]), 1), // Host rejection is before model mutation.
        ("turn/start", json!([skill]), 0),
        ("turn/start", json!([text, skill, skill]), 0),
        (
            "turn/start",
            json!([text, skill, {"type":"localImage","path":"/missing/must-not-read.png"}]),
            0,
        ),
        (
            "turn/start",
            json!([text, {"type":"mention","name":"plugin","path":"plugin://plugin@source"}]),
            0,
        ),
        (
            "turn/start",
            json!([text, {"type":"skill","name":"skill","path":"relative/SKILL.md"}]),
            0,
        ),
        ("turn/steer", json!([text, skill]), 0),
    ];
    let catalog = vec![ProviderModelEntry::try_new(
        "sonnet".into(),
        "Sonnet".into(),
        String::new(),
    )?];
    for (method, input, expected_preparations) in cases {
        let backend = ScriptedSessionBackend::new()?;
        let started = start(&backend).await?;
        let result = handle_app_server_turn_request(
            method,
            json!({"threadId":started["thread"]["id"],
            "model":"sonnet","expectedTurnId":"turn-1","input":input}),
            Arc::clone(&backend) as Arc<dyn SessionCommandPort>,
            Arc::clone(&backend) as Arc<dyn SessionEventHub>,
            backend.endpoint.clone(),
            ScriptedSessionBackend::actor()?,
            &catalog,
        )
        .await;
        assert!(
            matches!(
                result,
                Err(ThreadMethodError::SelectionRejected(
                    crate::SelectionFailure::UnsupportedForm
                ))
            ),
            "{method}: {result:?}"
        );
        assert_eq!(
            backend.selected_commands.lock().expect("test lock").len(),
            expected_preparations
        );
        assert!(
            backend
                .setting_commands
                .lock()
                .expect("test lock")
                .is_empty()
        );
        assert!(
            backend
                .prompt_commands
                .lock()
                .expect("test lock")
                .is_empty()
        );
        assert!(backend.steer_commands.lock().expect("test lock").is_empty());
    }
    Ok(())
}

#[tokio::test]
async fn native_capabilities_plain_dollar_text_bypasses_selected_resolution()
-> Result<(), Box<dyn std::error::Error>> {
    let backend = ScriptedSessionBackend::new()?;
    let started = start(&backend).await?;
    handle_app_server_turn_request(
        "turn/start",
        json!({"threadId":started["thread"]["id"],
        "input":[{"type":"text","text":"$HOME λ\n$skill"}]}),
        Arc::clone(&backend) as Arc<dyn SessionCommandPort>,
        Arc::clone(&backend) as Arc<dyn SessionEventHub>,
        backend.endpoint.clone(),
        ScriptedSessionBackend::actor()?,
        &[],
    )
    .await?;
    assert!(
        backend
            .selected_commands
            .lock()
            .expect("test lock")
            .is_empty()
    );
    assert_eq!(
        backend
            .prompt_commands
            .lock()
            .expect("test lock")
            .first()
            .ok_or("prompt")?
            .content,
        vec![CommandContent::text("$HOME λ\n$skill".into())?]
    );
    Ok(())
}

#[tokio::test]
async fn native_capabilities_selected_input_preserves_identity_before_prompt()
-> Result<(), Box<dyn std::error::Error>> {
    let backend = ScriptedSessionBackend::new()?;
    let actor = ScriptedSessionBackend::actor()?;
    let started = handle_app_server_thread_request(
        "thread/start",
        json!({"cwd":"/tmp"}),
        Arc::clone(&backend) as Arc<dyn SessionCommandPort>,
        Arc::clone(&backend) as Arc<dyn SessionEventHub>,
        backend.endpoint.clone(),
        actor.clone(),
        &[],
    )
    .await?;
    let native_content = vec![CommandContent::text("/native-skill  λ\n$HOME".into())?];
    *backend.selected_preparation.lock().expect("test lock") = SelectedPromptPreparation::Ready {
        content: native_content.clone(),
    };
    let response = handle_app_server_turn_request(
        "turn/start",
        json!({"threadId":started["thread"]["id"], "input":[
            {"type":"text","text":"$skill  λ\n$HOME","text_elements":[]},
            {"type":"skill","name":"skill","path":"/tmp/native/SKILL.md"}
        ]}),
        Arc::clone(&backend) as Arc<dyn SessionCommandPort>,
        Arc::clone(&backend) as Arc<dyn SessionEventHub>,
        backend.endpoint.clone(),
        actor.clone(),
        &[],
    )
    .await?;
    assert_eq!(response["turn"]["status"], "inProgress");
    let selected = backend.selected_commands.lock().expect("test lock");
    assert_eq!(selected.len(), 1);
    let selected = selected.first().ok_or("missing preparation")?;
    assert_eq!(selected.actor, actor);
    assert_eq!(selected.target, backend.session);
    assert_eq!(
        selected.content,
        vec![CommandContent::text("$skill  λ\n$HOME".into())?]
    );
    assert!(
        matches!(selected.selections.as_slice(), [SelectedItem::Skill { submitted_name, submitted_path }]
        if submitted_name.as_str() == "skill" && submitted_path.as_path() == Path::new("/tmp/native/SKILL.md"))
    );
    let prompts = backend.prompt_commands.lock().expect("test lock");
    assert_eq!(prompts.len(), 1);
    let prompt = prompts.first().ok_or("missing prompt")?;
    assert_eq!(prompt.content, native_content);
    assert_eq!(prompt.target, selected.target);
    assert_eq!(prompt.actor, selected.actor);
    assert_eq!(prompt.input_id, selected.input_id);
    assert!(backend.steer_commands.lock().expect("test lock").is_empty());
    Ok(())
}
