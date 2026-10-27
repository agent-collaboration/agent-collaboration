//! Read-only provider inventory from Router's durable Session records.

use crate::collaboration_application::{
    ProviderInventoryFailure, ProviderInventoryFailureKind, ResultByteBudget,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use collaboration_protocol::{
    ChannelDescription, HostedProviderOrigin, NativeSessionScope, NativeSessionSource,
    NativeSessionView, ProviderSessionListParams, ProviderSessionListResult, ProviderSessionState,
    ProviderSessionSummary, UuidIdentity,
};
use serde::{Deserialize, Serialize};
use session_event_model::SessionState;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProviderInventoryCursor {
    endpoint: collaboration_protocol::EndpointRef,
    view: String,
    scope: NativeSessionScope,
    source: NativeSessionSource,
    query: Option<String>,
    sort_time_ms: i64,
    session_id: String,
    origin: ProviderInventoryOrigin,
    service_epoch: UuidIdentity,
    expires_at: i64,
}

fn failure(kind: ProviderInventoryFailureKind) -> ProviderInventoryFailure {
    ProviderInventoryFailure::Unavailable(kind)
}

fn invalid(message: &'static str) -> ProviderInventoryFailure {
    ProviderInventoryFailure::InvalidRequest(message)
}

fn view_name(view: NativeSessionView) -> &'static str {
    match view {
        NativeSessionView::Stored => "stored",
        NativeSessionView::Loaded => "loaded",
        NativeSessionView::Active => "active",
    }
}

fn state_tag(state: &SessionState) -> ProviderSessionState {
    match state {
        SessionState::Unloaded => ProviderSessionState::Unloaded,
        SessionState::Idle => ProviderSessionState::Idle,
        SessionState::Running => ProviderSessionState::Running,
        SessionState::RequiresAction { .. } => ProviderSessionState::RequiresAction,
        SessionState::AuthenticationRequired => ProviderSessionState::AuthenticationRequired,
        SessionState::Closed => ProviderSessionState::Closed,
    }
}

fn view_matches(view: NativeSessionView, state: ProviderSessionState) -> bool {
    match view {
        NativeSessionView::Stored => true,
        NativeSessionView::Loaded => !matches!(
            state,
            ProviderSessionState::Unloaded | ProviderSessionState::Closed
        ),
        NativeSessionView::Active => matches!(
            state,
            ProviderSessionState::Running | ProviderSessionState::RequiresAction
        ),
    }
}

fn scope_matches(scope: &NativeSessionScope, cwd: &str) -> bool {
    let candidate = std::path::Path::new(cwd);
    let candidate_values = codex_native_integration::path_sql_values(candidate);
    match scope {
        NativeSessionScope::Any => true,
        NativeSessionScope::Cwd { path } => candidate_values
            .iter()
            .any(|value| codex_native_integration::path_sql_values(path).contains(value)),
        NativeSessionScope::Checkout { root } => {
            let root_values = codex_native_integration::path_sql_values(root);
            candidate_values.iter().any(|candidate| {
                root_values.iter().any(|root| {
                    candidate == root
                        || candidate
                            .strip_prefix(root)
                            .is_some_and(|suffix| suffix.starts_with('/'))
                })
            })
        }
        NativeSessionScope::Repo {
            live_roots,
            normalized_origin,
            basename,
            fallback_cwd,
        } => {
            let identity = codex_native_integration::RepositoryIdentity {
                normalized_origin: normalized_origin.clone(),
                live_roots: live_roots.clone(),
                repository_basename: basename.clone(),
                fallback_cwd: fallback_cwd.clone(),
            };
            codex_native_integration::repository_contains_session(&identity, None, candidate)
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum ProviderInventoryOrigin {
    Managed,
    NativePeer,
    NativeBackground,
}

struct ProviderInventoryRow {
    sort_time_ms: i64,
    session_id: String,
    origin: ProviderInventoryOrigin,
    summary: ProviderSessionSummary,
}

fn cursor_after(row: &ProviderInventoryRow, cursor: &ProviderInventoryCursor) -> bool {
    row.sort_time_ms < cursor.sort_time_ms
        || (row.sort_time_ms == cursor.sort_time_ms
            && (row.session_id.as_str(), row.origin) > (cursor.session_id.as_str(), cursor.origin))
}

/// Pages a provider endpoint's hosted sessions, or Claude Code's live terminal sessions.
pub struct ProviderSessionInventory {
    identity: crate::RouterIdentity,
    directory: collaboration_runtime::EndpointDirectory,
    provider_operations: Option<std::sync::Arc<tokio::sync::Mutex<collaboration_runtime::ProviderOperationStore>>>,
    provider_session_hub: Option<std::sync::Arc<dyn collaboration_contracts::SessionEventHub>>,
    display_names: collaboration_runtime::SessionDisplayNameCache,
    claude_code_sessions:
        Option<std::sync::Arc<claude_code_peer_messaging::ClaudeCodeSessionRegistry>>,
    claude_background_jobs:
        Option<std::sync::Arc<claude_code_peer_messaging::NativeClaudeJobReader>>,
}
impl ProviderSessionInventory {
    pub fn new(
        identity: crate::RouterIdentity,
        directory: collaboration_runtime::EndpointDirectory,
        provider_operations: Option<
            std::sync::Arc<tokio::sync::Mutex<collaboration_runtime::ProviderOperationStore>>,
        >,
        provider_session_hub: Option<std::sync::Arc<dyn collaboration_contracts::SessionEventHub>>,
        display_names: collaboration_runtime::SessionDisplayNameCache,
        claude_code_sessions: Option<
            std::sync::Arc<claude_code_peer_messaging::ClaudeCodeSessionRegistry>,
        >,
        claude_background_jobs: Option<
            std::sync::Arc<claude_code_peer_messaging::NativeClaudeJobReader>,
        >,
    ) -> Self {
        Self {
            identity,
            directory,
            provider_operations,
            provider_session_hub,
            display_names,
            claude_code_sessions,
            claude_background_jobs,
        }
    }
    pub(crate) async fn list(
        &self,
        params: ProviderSessionListParams,
        budget: ResultByteBudget,
    ) -> Result<ProviderSessionListResult, ProviderInventoryFailure> {
        if !(1..=100).contains(&params.page_size) {
            return Err(invalid("Provider session page size must be 1..100"));
        }
        let is_claude = String::from(params.endpoint.endpoint_id.clone()) == "claude-local";
        if !is_claude && !matches!(params.source, NativeSessionSource::All) {
            return Err(invalid("Provider sessions carry no source; use source all"));
        }
        if is_claude && matches!(params.source, NativeSessionSource::Subagents) {
            return Err(invalid(
                "Claude Code terminal discovery supports source interactive or all",
            ));
        }
        if params.endpoint.service_id != *self.identity.service_id() {
            return Err(failure(ProviderInventoryFailureKind::WrongService));
        }
        let description = match self.directory.read_endpoint(&params.endpoint) {
            Ok(Some(description)) => description,
            Ok(None) => return Err(failure(ProviderInventoryFailureKind::EndpointNotFound)),
            Err(_) => return Err(failure(ProviderInventoryFailureKind::Unavailable)),
        };
        if !is_claude
            && !description
                .channels
                .iter()
                .any(|channel| matches!(channel, ChannelDescription::ExternalProvider { .. }))
        {
            return Err(failure(ProviderInventoryFailureKind::UnsupportedCapability));
        }
        let cursor = match &params.cursor {
            None => None,
            Some(encoded) => {
                if encoded.is_empty() || encoded.len() > 1024 {
                    return Err(invalid("Invalid provider session inventory cursor"));
                }
                let decoded = URL_SAFE_NO_PAD.decode(encoded).ok().and_then(|bytes| {
                    serde_json::from_slice::<ProviderInventoryCursor>(&bytes).ok()
                });
                let Some(cursor) = decoded.filter(|cursor| {
                    cursor.endpoint == params.endpoint
                        && cursor.service_epoch == *self.identity.service_epoch()
                        && cursor.view == view_name(params.view)
                        && cursor.scope == params.scope
                        && cursor.source == params.source
                        && cursor.query == params.query
                        && cursor.expires_at > chrono::Utc::now().timestamp()
                }) else {
                    return Err(invalid("Invalid provider session inventory cursor"));
                };
                Some(cursor)
            }
        };
        let inventory = if matches!(params.source, NativeSessionSource::Interactive) {
            Vec::new()
        } else {
            let Some(store) = self.provider_operations.as_ref() else {
                return Err(failure(ProviderInventoryFailureKind::Unavailable));
            };
            match store.lock().await.list_sessions(&params.endpoint).await {
                Ok(inventory) => inventory,
                Err(_) => return Err(failure(ProviderInventoryFailureKind::Unavailable)),
            }
        };
        let mut rows: Vec<ProviderInventoryRow> = Vec::new();
        let mut skipped_records = None;
        let mut native_coverage = None;
        if is_claude
            && !(matches!(params.view, NativeSessionView::Stored)
                && matches!(params.source, NativeSessionSource::All))
        {
            let observed = crate::native_claude_catalogue::read_native_catalogue(
                self.claude_code_sessions.as_ref(),
                self.claude_background_jobs.as_ref(),
                &params.endpoint,
                params.view,
            )
            .await?;
            native_coverage = Some(observed.coverage);
            skipped_records = observed.skipped_records;
            for summary in observed.sessions {
                let (sort_time_ms, name, working_directory, origin) = match &summary {
                    ProviderSessionSummary::ClaudeCodeInteractive {
                        updated_at,
                        name,
                        working_directory,
                        ..
                    } => (
                        updated_at.saturating_mul(1000),
                        name,
                        working_directory,
                        ProviderInventoryOrigin::NativePeer,
                    ),
                    ProviderSessionSummary::ClaudeCodeBackground {
                        started_at,
                        peer,
                        name,
                        working_directory,
                        ..
                    } => (
                        peer.as_ref()
                            .map_or(*started_at, |peer| peer.updated_at)
                            .saturating_mul(1000),
                        name,
                        working_directory,
                        ProviderInventoryOrigin::NativeBackground,
                    ),
                    ProviderSessionSummary::HostedProvider { .. } => {
                        return Err(invalid("managed record in native catalogue"));
                    }
                };
                let session_id = String::from(summary.target().session_id.clone());
                let cwd = String::from(working_directory.clone());
                if !scope_matches(&params.scope, &cwd)
                    || params.query.as_ref().is_some_and(|query| {
                        let query = query.to_lowercase();
                        !session_id.to_lowercase().contains(&query)
                            && !cwd.to_lowercase().contains(&query)
                            && !name
                                .as_ref()
                                .is_some_and(|name| name.to_lowercase().contains(&query))
                    })
                {
                    continue;
                }
                if let Some(name) = name {
                    self.display_names.remember(summary.target().clone(), name);
                } else {
                    self.display_names.forget(summary.target().clone());
                }
                rows.push(ProviderInventoryRow {
                    sort_time_ms,
                    session_id,
                    origin,
                    summary,
                });
            }
        }
        for record in inventory {
            let session_id = String::from(record.target.session_id.clone());
            let cwd = String::from(record.working_directory.clone());
            if !scope_matches(&params.scope, &cwd)
                || params.query.as_ref().is_some_and(|query| {
                    let query = query.to_lowercase();
                    !session_id.to_lowercase().contains(&query)
                        && !cwd.to_lowercase().contains(&query)
                })
            {
                continue;
            }
            let state = if let Some(hub) = self.provider_session_hub.as_ref() {
                let hub_session = match serde_json::to_value(&record.target)
                    .ok()
                    .and_then(|value| serde_json::from_value(value).ok())
                {
                    Some(session) => session,
                    None => return Err(failure(ProviderInventoryFailureKind::Unavailable)),
                };
                match hub.state(hub_session).await {
                    Ok(state) => state_tag(&state),
                    Err(_) => return Err(failure(ProviderInventoryFailureKind::Unavailable)),
                }
            } else {
                ProviderSessionState::Unloaded
            };
            if !view_matches(params.view, state) {
                continue;
            }
            let approver = record.approver;
            rows.push(ProviderInventoryRow {
                sort_time_ms: record.updated_at_ms,
                session_id,
                origin: ProviderInventoryOrigin::Managed,
                summary: ProviderSessionSummary::HostedProvider {
                    origin: is_claude.then_some(HostedProviderOrigin::HostedProvider),
                    target: record.target,
                    working_directory: record.working_directory,
                    updated_at: record.updated_at_ms.div_euclid(1_000),
                    state,
                    requested_policy: record.requested_policy,
                    approver,
                    created_by: record.created_by,
                },
            });
        }
        rows.sort_by(|left, right| {
            right
                .sort_time_ms
                .cmp(&left.sort_time_ms)
                .then_with(|| left.session_id.cmp(&right.session_id))
                .then_with(|| left.origin.cmp(&right.origin))
        });
        if let Some(cursor) = &cursor {
            rows.retain(|row| cursor_after(row, cursor));
        }
        let has_more = rows.len() > params.page_size as usize;
        rows.truncate(params.page_size as usize);
        let next_cursor = if has_more {
            let Some(previous) = rows.last() else {
                return Err(failure(ProviderInventoryFailureKind::Unavailable));
            };
            let cursor = ProviderInventoryCursor {
                endpoint: params.endpoint.clone(),
                view: view_name(params.view).to_owned(),
                scope: params.scope.clone(),
                source: params.source,
                query: params.query.clone(),
                sort_time_ms: previous.sort_time_ms,
                session_id: previous.session_id.clone(),
                origin: previous.origin,
                service_epoch: self.identity.service_epoch().clone(),
                expires_at: chrono::Utc::now().timestamp() + 60,
            };
            let encoded = serde_json::to_vec(&cursor)
                .ok()
                .map(|bytes| URL_SAFE_NO_PAD.encode(bytes));
            if encoded.as_ref().is_none_or(|cursor| cursor.len() > 1024) {
                return Err(failure(ProviderInventoryFailureKind::ResponseTooLarge));
            }
            encoded
        } else {
            None
        };
        let observed_at = match collaboration_protocol::ObservationTimestamp::try_from(
            chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        ) {
            Ok(time) => time,
            Err(_) => return Err(failure(ProviderInventoryFailureKind::Unavailable)),
        };
        let result = ProviderSessionListResult {
            endpoint: params.endpoint,
            observed_at,
            sessions: rows.into_iter().map(|row| row.summary).collect(),
            native_coverage,
            skipped_records,
            next_cursor,
        };
        if budget.admits(&result) {
            Ok(result)
        } else {
            Err(failure(ProviderInventoryFailureKind::ResponseTooLarge))
        }
    }
}
