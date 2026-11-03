//! Complete native discovery and transcript facts used to select a safe resume.
use super::{
    SessionsCommandError,
    claude_launch_target::{ClaudeLaunchTarget, NativeClaudeInventory},
};
use collaboration_client::protocol::{NativeSessionView, ProviderSessionSummary};
use std::{collections::BTreeMap, path::PathBuf};

impl ClaudeLaunchTarget {
    pub(super) async fn complete_resume_inventory(
        &self,
    ) -> Result<NativeClaudeInventory, SessionsCommandError> {
        let mut combined = NativeClaudeInventory {
            sessions: Vec::new(),
            read_notice: None,
            conflicts: Vec::new(),
        };
        for view in [NativeSessionView::Loaded, NativeSessionView::Stored] {
            let inventory = self.native_session_inventory(view, usize::MAX).await
                .map_err(|message| SessionsCommandError::ClaudeLaunch(format!("Could not verify the Claude session: {message}. Check claude agents before opening it again.")))?;
            if let Some(notice) = inventory.read_notice {
                return Err(SessionsCommandError::ClaudeLaunch(format!(
                    "Claude discovery is incomplete: {notice}. Check claude agents before opening this session."
                )));
            }
            for summary in inventory.sessions {
                if !combined
                    .sessions
                    .iter()
                    .any(|existing| existing.target() == summary.target())
                {
                    combined.sessions.push(summary);
                }
            }
            combined.conflicts.extend(inventory.conflicts);
        }
        Ok(combined)
    }

    pub(super) async fn resume_candidates(
        &self,
        inventory: &NativeClaudeInventory,
    ) -> Result<BTreeMap<String, String>, SessionsCommandError> {
        let mut candidates = BTreeMap::new();
        for record in self.stored_transcript_records().await? {
            if let Some(id) = record.get("sessionId").and_then(serde_json::Value::as_str) {
                candidates.insert(
                    id.to_owned(),
                    record
                        .get("project")
                        .and_then(serde_json::Value::as_str)
                        .map_or_else(
                            || "Untitled Claude session".to_owned(),
                            |project| format!("Untitled Claude session (project: {project})"),
                        ),
                );
            }
        }
        for summary in &inventory.sessions {
            let name = match summary {
                ProviderSessionSummary::ClaudeCodeInteractive { name, .. }
                | ProviderSessionSummary::ClaudeCodeBackground { name, .. } => name.clone(),
                ProviderSessionSummary::HostedProvider { .. } => None,
            };
            candidates.insert(
                String::from(summary.target().session_id.clone()),
                name.unwrap_or_else(|| "Untitled Claude session".to_owned()),
            );
        }
        for conflict in &inventory.conflicts {
            candidates.insert(
                String::from(conflict.target.session_id.clone()),
                "Duplicate live peers".to_owned(),
            );
        }
        Ok(candidates)
    }

    pub(super) async fn transcript_resume_directory(
        &self,
        session_id: &str,
    ) -> Result<PathBuf, SessionsCommandError> {
        let projects = self.claude_projects_directory.clone();
        let session_id = session_id.to_owned();
        tokio::task::spawn_blocking(move || {
            let projects =
                projects.ok_or_else(|| "Claude home directory is unavailable".to_owned())?;
            super::claude_transcript_inventory::recorded_resume_directory(&projects, &session_id)
        })
        .await
        .map_err(|error| {
            SessionsCommandError::ClaudeLaunch(format!("Claude transcript read failed: {error}"))
        })?
        .map_err(SessionsCommandError::ClaudeLaunch)
    }
}
