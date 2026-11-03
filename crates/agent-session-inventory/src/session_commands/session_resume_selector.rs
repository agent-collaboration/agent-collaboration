//! Resolve explicit CLI identifiers without guessing a provider or starting a process.
use super::{
    SessionRecordQuery, SessionsCommand, SessionsCommandError, SessionsProvider,
    claude_launch_target::ClaudeLaunchTarget, load_session_records_for_query_with_identity,
    validate_exact_uuid_session_id,
};
use collaboration_client::protocol::ProviderSessionSummary;
use std::path::Path;

pub async fn resolve_session_resume_selector(
    command: &mut SessionsCommand,
    codex_home: &Path,
    current_directory: &Path,
    claude: Option<&ClaudeLaunchTarget>,
) -> Result<(), SessionsCommandError> {
    let Some(selector) = command.id.as_deref() else {
        return Ok(());
    };
    let exact = validate_exact_uuid_session_id(selector).is_ok();
    if exact && command.provider != SessionsProvider::Any {
        return Ok(());
    }
    let mut candidates = Vec::new();
    if let Some(claude) = claude {
        let inventory = claude.complete_resume_inventory().await?;
        if !exact {
            let handles = inventory.sessions.iter().filter(|summary| matches!(summary,
                ProviderSessionSummary::ClaudeCodeBackground { job_handle, .. } if String::from(job_handle.clone()).eq_ignore_ascii_case(selector)
            )).collect::<Vec<_>>();
            if handles.len() == 1 {
                command.provider = SessionsProvider::ClaudeCode;
                command.id = Some(selector.to_ascii_lowercase());
                return Ok(());
            }
            if handles.len() > 1 {
                return Err(SessionsCommandError::ResumeSelection(
                    "Claude job handle is ambiguous; check claude agents".to_owned(),
                ));
            }
        }
        for (id, title) in claude.resume_candidates(&inventory).await? {
            if matches_selector(&id, selector, exact) {
                candidates.push((true, id, title));
            }
        }
    }
    if !command.provider.is_claude_code() {
        match tokio::fs::metadata(codex_home.join("state_5.sqlite")).await {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(SessionsCommandError::ResumeSelection(format!(
                    "Could not read Codex inventory: {error}"
                )));
            }
            Ok(_) => {
                let query = if exact {
                    SessionRecordQuery::for_session_id(selector)
                } else {
                    SessionRecordQuery::for_session_prefix()
                };
                for record in load_session_records_for_query_with_identity(
                    query,
                    codex_home,
                    current_directory,
                    None,
                )
                .await?
                {
                    if matches_selector(&record.session_id, selector, exact) {
                        let title = record
                            .name
                            .or(record.title)
                            .or(record.first_user_message)
                            .unwrap_or_else(|| "Untitled Codex session".to_owned());
                        candidates.push((false, record.session_id, title));
                    }
                }
            }
        }
    }
    match candidates.as_slice() {
        [] => Err(SessionsCommandError::ResumeSelection(format!(
            "no session with that ID: {selector}"
        ))),
        [(is_claude, id, _)] => {
            if *is_claude {
                command.provider = SessionsProvider::ClaudeCode;
            }
            command.id = Some(id.clone());
            Ok(())
        }
        _ => {
            candidates.sort();
            let mut message =
                format!("Session ID {selector} is ambiguous. Use --provider and a full UUID:\n");
            for (is_claude, id, title) in candidates {
                let provider = if is_claude { "claude" } else { "codex" };
                // Titles are terminal display data, never an escape/control channel.
                let title = title
                    .chars()
                    .map(|character| {
                        if character.is_control() {
                            ' '
                        } else {
                            character
                        }
                    })
                    .collect::<String>();
                message.push_str(&format!("  {provider}  {id}  {title}\n"));
            }
            Err(SessionsCommandError::ResumeSelection(message))
        }
    }
}

fn matches_selector(id: &str, selector: &str, exact: bool) -> bool {
    if validate_exact_uuid_session_id(id).is_err() {
        return false;
    }
    if exact {
        id.eq_ignore_ascii_case(selector)
    } else {
        id.replace('-', "")
            .to_ascii_lowercase()
            .starts_with(&selector.replace('-', "").to_ascii_lowercase())
    }
}
