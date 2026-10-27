//! Reads a local session's repository without exposing unrelated session capabilities.
use message_board::{RepositoryRef, SessionRef, normalize_git_origin_url};
use std::path::{Path, PathBuf};
pub struct SessionContextReader {
    identity: crate::RouterIdentity,
    native: super::session_native_control::NativeSessionControl,
    provider_operations: Option<std::sync::Arc<tokio::sync::Mutex<crate::ProviderOperationStore>>>,
    claude_code_sessions:
        Option<std::sync::Arc<claude_code_peer_messaging::ClaudeCodeSessionRegistry>>,
}
impl SessionContextReader {
    pub fn new(
        identity: crate::RouterIdentity,
        native: super::session_native_control::NativeSessionControl,
        provider_operations: Option<
            std::sync::Arc<tokio::sync::Mutex<crate::ProviderOperationStore>>,
        >,
        claude_code_sessions: Option<
            std::sync::Arc<claude_code_peer_messaging::ClaudeCodeSessionRegistry>,
        >,
    ) -> Self {
        Self {
            identity,
            native,
            provider_operations,
            claude_code_sessions,
        }
    }
    pub(crate) async fn repository(&self, session: &SessionRef) -> Option<RepositoryRef> {
        let target = serde_json::to_value(session)
            .and_then(serde_json::from_value::<collaboration_protocol::SessionRef>)
            .ok()?;
        if target.endpoint.service_id != *self.identity.service_id() {
            return None;
        }
        let path = match session.endpoint.endpoint_id.as_str() {
            "codex-local" => {
                let result = self
                    .native
                    .codex_session_inspect(
                        collaboration_protocol::NativeInspectParams { target },
                        crate::collaboration_application::API_RESULT_BUDGET,
                    )
                    .await
                    .ok()?;
                PathBuf::from(result.thread.get("cwd")?.as_str()?)
            }
            "claude-local" | "cursor-local" => {
                let mut live_path = None;
                if session.endpoint.endpoint_id.as_str() == "claude-local"
                    && let Some(registry) = self.claude_code_sessions.clone()
                {
                    let live = tokio::task::spawn_blocking(move || registry.live_sessions())
                        .await
                        .ok()?
                        .ok()?;
                    live_path = live
                        .sessions
                        .into_iter()
                        .find(|row| row.session_id == target.session_id)
                        .map(|row| row.cwd);
                }
                match live_path {
                    Some(path) => path,
                    None => PathBuf::from(String::from(
                        self.provider_operations
                            .as_ref()?
                            .lock()
                            .await
                            .session_record(&target)
                            .await
                            .ok()??
                            .working_directory,
                    )),
                }
            }
            _ => return None,
        };
        let path = tokio::fs::canonicalize(path).await.ok()?;
        if let Some(origin) = context_git_stdout(&path, &["remote", "get-url", "origin"]).await
            && let Some(normalized) = normalize_git_origin_url(&origin)
        {
            return Some(RepositoryRef::Origin {
                normalized_origin: normalized.try_into().ok()?,
            });
        }
        let common = context_git_stdout(
            &path,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )
        .await?;
        let common = tokio::fs::canonicalize(common).await.ok()?;
        Some(RepositoryRef::Local {
            service_id: session.endpoint.service_id.clone(),
            common_directory: common.to_str()?.to_owned().try_into().ok()?,
        })
    }
}
async fn context_git_stdout(path: &Path, arguments: &[&str]) -> Option<String> {
    let output = tokio::process::Command::new("git")
        .arg("-C")
        .arg(path)
        .args(arguments)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8(output.stdout).ok()?.trim().to_owned();
    (!value.is_empty()).then_some(value)
}
