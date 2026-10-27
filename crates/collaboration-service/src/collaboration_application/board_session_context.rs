//! Session context reads assignments and scope acknowledgements from associated projects.
use super::{BoardOperations, presence_for_reader};
use message_board::{
    BoardError, FactKind, Identity, ProjectId, Record, RepositoryRef, SessionContext,
    SessionContextRequest, SessionRef, SessionRunningState, UnavailableReason,
    normalize_git_origin_url,
};
use message_board_storage::StoreSchemaState;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

impl BoardOperations<'_> {
    pub(super) async fn card_session_state(&self, actor: &Identity) -> SessionRunningState {
        if !matches!(actor, Identity::Session { .. }) {
            return SessionRunningState::NotSession {};
        }
        let Some(probe) = &self.identity.subscription_presence else {
            return SessionRunningState::Unavailable {
                reason: "Host presence unavailable".to_owned(),
            };
        };
        match presence_for_reader(actor, probe).await {
            collaboration_protocol::ThreadSubscriptionPresence::Running {} => {
                SessionRunningState::Running {}
            }
            collaboration_protocol::ThreadSubscriptionPresence::Wakeable {} => {
                SessionRunningState::Wakeable {}
            }
            collaboration_protocol::ThreadSubscriptionPresence::Unreachable { reason } => {
                SessionRunningState::Unavailable { reason }
            }
        }
    }
    pub async fn view_session_context(
        &self,
        request: SessionContextRequest,
    ) -> Result<SessionContext, BoardError> {
        self.context_payload(&request.session)
            .await
            .map_err(|_| BoardError::board_unavailable())
    }
    async fn context_payload(
        &self,
        session: &SessionRef,
    ) -> Result<SessionContext, UnavailableReason> {
        let projects = self.context_projects(session).await?;
        let reader = Identity::Session {
            session: session.clone(),
        };
        let mut assignments = Vec::new();
        let mut acknowledged_scopes = Vec::new();
        for project in projects {
            if let Some(reason) = self.context_project_unavailable(&project).await {
                return Err(reason);
            }
            let unavailable = || UnavailableReason::StoreUnavailable {
                project: project.clone(),
            };
            let admission = self
                .identity
                .project_admission
                .as_ref()
                .ok_or(UnavailableReason::BoardUnreachable {})?;
            let store = admission
                .registry
                .read_project(&project)
                .await
                .map_err(|_| unavailable())?
                .ok_or_else(unavailable)?;
            let mut snapshot = store.snapshot().await.map_err(|_| unavailable())?;
            let role_keys = snapshot
                .active_actor_role_keys(&reader)
                .await
                .map_err(|_| unavailable())?;
            let roles = snapshot
                .record_batch(&role_keys)
                .await
                .map_err(|_| unavailable())?
                .into_iter()
                .map(|row| row.decode())
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| unavailable())?;
            assignments.extend(roles.into_iter().filter_map(|record| match record {
                Record::RoleAssignment(role)
                    if role.ended.is_none()
                        && role.holder.declared_identity().as_ref() == Some(&reader) =>
                {
                    Some(role.assignment())
                }
                _ => None,
            }));
            let mut after = String::new();
            loop {
                let mut keys = snapshot
                    .scope_acknowledgement_page_keys(session, &after, 101)
                    .await
                    .map_err(|_| unavailable())?;
                let more = keys.len() > 100;
                keys.truncate(100);
                if let Some(last) = keys.last() {
                    after = last.id.as_str().to_owned();
                }
                for stored in snapshot
                    .record_batch(&keys)
                    .await
                    .map_err(|_| unavailable())?
                {
                    let Record::Fact(fact) = stored.decode().map_err(|_| unavailable())? else {
                        return Err(unavailable());
                    };
                    if !matches!(&fact.kind, FactKind::ScopeAcknowledgement { session: recorded, .. } if recorded == session)
                    {
                        return Err(unavailable());
                    }
                    acknowledged_scopes.push(fact.fact_id);
                }
                if !more {
                    break;
                }
            }
            snapshot.finish().await.map_err(|_| unavailable())?;
        }
        Ok(SessionContext::Context {
            session: session.clone(),
            assignments,
            acknowledged_scopes,
        })
    }
    async fn context_projects(
        &self,
        session: &SessionRef,
    ) -> Result<Vec<ProjectId>, UnavailableReason> {
        let unavailable = || UnavailableReason::SessionMetadataUnavailable {
            session: session.clone(),
        };
        let repository = self
            .context_repository(session)
            .await
            .ok_or_else(unavailable)?;
        let admission = self
            .identity
            .project_admission
            .as_ref()
            .ok_or(UnavailableReason::BoardUnreachable {})?;
        let mut snapshot = admission
            .fleet
            .read_snapshot()
            .await
            .map_err(|_| UnavailableReason::BoardUnreachable {})?;
        let mut projects = BTreeSet::new();
        let mut after = String::new();
        loop {
            let mut keys = snapshot
                .list_page_keys(
                    message_board_storage::FleetListFilter::Workspaces {
                        repository: Some(&repository),
                    },
                    &after,
                    101,
                )
                .await
                .map_err(|_| UnavailableReason::BoardUnreachable {})?;
            let more = keys.len() > 100;
            keys.truncate(100);
            if let Some(last) = keys.last() {
                after = last.id.as_str().to_owned();
            }
            for stored in snapshot
                .record_batch(&keys)
                .await
                .map_err(|_| UnavailableReason::BoardUnreachable {})?
            {
                let Record::Workspace(workspace) = stored
                    .decode()
                    .map_err(|_| UnavailableReason::BoardUnreachable {})?
                else {
                    return Err(UnavailableReason::BoardUnreachable {});
                };
                if !workspace.repositories.contains(&repository) {
                    return Err(UnavailableReason::BoardUnreachable {});
                }
                projects.extend(
                    snapshot
                        .workspace_project_ids(&workspace.workspace_id)
                        .await
                        .map_err(|_| UnavailableReason::BoardUnreachable {})?,
                );
                projects.extend(
                    admission
                        .fleet
                        .pending_workspace_projects(&workspace.workspace_id, &admission.registry)
                        .await
                        .map_err(|_| UnavailableReason::BoardUnreachable {})?,
                );
            }
            if !more {
                break;
            }
        }
        snapshot
            .finish()
            .await
            .map_err(|_| UnavailableReason::BoardUnreachable {})?;
        let mut selected = Vec::new();
        for project in projects {
            let rejected = admission
                .registry
                .is_rejected_project(&project)
                .await
                .map_err(|_| UnavailableReason::StoreUnavailable {
                    project: project.clone(),
                })?;
            if !rejected {
                selected.push(project);
            }
        }
        Ok(selected)
    }
    async fn context_repository(&self, session: &SessionRef) -> Option<RepositoryRef> {
        let target = serde_json::to_value(session)
            .and_then(serde_json::from_value::<collaboration_protocol::SessionRef>)
            .ok()?;
        if target.endpoint.service_id != self.identity.service_id {
            return None;
        }
        let path = match session.endpoint.endpoint_id.as_str() {
            "codex-local" => {
                let result =
                    crate::collaboration_application::SessionOperations::new(self.identity)
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
                    && let Some(registry) = self.identity.claude_code_sessions.clone()
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
                        self.identity
                            .provider_operations
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
    async fn context_project_unavailable(&self, project: &ProjectId) -> Option<UnavailableReason> {
        let Some(admission) = &self.identity.project_admission else {
            return Some(UnavailableReason::BoardUnreachable {});
        };
        let schema = match admission.registry.read_project(project).await {
            Ok(Some(store)) => store.schema_state().await,
            _ => {
                return Some(UnavailableReason::StoreUnavailable {
                    project: project.clone(),
                });
            }
        };
        match schema {
            Ok(StoreSchemaState::Ready { .. }) => None,
            Ok(StoreSchemaState::ReadOnly { .. }) => Some(UnavailableReason::SchemaNotReady {
                project: project.clone(),
            }),
            _ => Some(UnavailableReason::StoreUnavailable {
                project: project.clone(),
            }),
        }
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
