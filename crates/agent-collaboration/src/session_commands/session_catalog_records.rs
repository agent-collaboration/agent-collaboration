//! CLI and picker projection of reusable stored-session records.

use super::{
    SESSION_CONVERSATION_SNIPPET_MAX_CHARS, display_title_from_session_fields,
    format_recency_at_ms, normalize_path, session_context_from_cwd, truncate_end,
};
use collaboration_client::protocol::{
    ChannelDescription, EndpointDescription, EndpointRef, NativeSessionSource, ProviderKind,
    ProviderSessionState, ProviderSessionSummary, SessionRef,
};
use collaboration_client::session_catalog::{
    SessionHistorySource, StoredSessionRecord, read_session_conversation_history,
};
use std::path::Path;

use crate::picker_runtime_status::PickerRuntimeStatus;
use crate::presentation::session_picker::SessionProviderFamily;

pub(super) type SessionRecord = StoredSessionRecord;
pub(crate) type SessionConversationSource = SessionHistorySource;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) enum SessionPickerIdentity {
    LocalCodex(String),
    HostedCodex(SessionRef),
    HostedProvider(SessionRef),
    NativeClaude(SessionRef),
}

impl SessionPickerIdentity {
    pub(crate) fn is_provider(&self) -> bool {
        matches!(self, Self::HostedProvider(_) | Self::NativeClaude(_))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SessionRowProvenance {
    LocalHomeCatalog,
    DefaultAttributed,
    ObservedHosted,
    ObservedProvider,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SessionPickerRecord {
    pub(crate) identity: SessionPickerIdentity,
    pub(crate) provider_family: SessionProviderFamily,
    pub(crate) provenance: SessionRowProvenance,
    pub(crate) source_context: Option<crate::presentation::session_picker::PickerSourceContext>,
    pub(crate) machine_display_label: Option<String>,
    pub(crate) endpoint_label: Option<String>,
    pub(crate) provider_state: Option<ProviderSessionState>,
    pub(crate) native_background: Option<NativeBackgroundRecord>,
    pub(crate) native_peer: Option<collaboration_client::protocol::NativeClaudePeerObservation>,
    pub(crate) native_inventory_view: Option<crate::picker_runtime_status::NativeCatalogueView>,
    pub(crate) session_id: String,
    pub(crate) title: String,
    pub(crate) full_title: String,
    pub(crate) explicit_name: Option<String>,
    pub(crate) recency: String,
    pub(crate) created: String,
    pub(crate) recency_at_ms: Option<i64>,
    pub(crate) created_at_ms: Option<i64>,
    pub(crate) branch: String,
    pub(crate) persisted_branch: String,
    pub(crate) context: String,
    pub(crate) cwd: Option<String>,
    pub(crate) normalized_cwd: Option<String>,
    pub(crate) git_origin_url: Option<String>,
    pub(crate) provider: Option<String>,
    pub(crate) model: Option<String>,
    pub(crate) reasoning_effort: Option<String>,
    pub(crate) preview: Option<String>,
    pub(crate) first_user_message: String,
    pub(crate) conversation: SessionConversationPreview,
    pub(crate) conversation_source: Option<SessionConversationSource>,
    pub(crate) source: Option<String>,
    pub(crate) thread_source: Option<String>,
    pub(crate) native_source: Option<NativeSessionSource>,
    pub(crate) runtime_status: PickerRuntimeStatus,
}

#[path = "native_catalogue_record.rs"]
mod native_catalogue_record;
use native_catalogue_record::{NativeBackgroundRecord, native_peer_runtime_status};

enum SessionPathSpace {
    InvokingMachine,
    SourceMachine,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SessionConversationPreview {
    pub(crate) snippets: Vec<String>,
    pub(crate) unavailable_reason: Option<String>,
}

impl SessionPickerRecord {
    pub(crate) fn native_restore_eligible(&self) -> bool {
        !self.native_background.as_ref().is_some_and(|job| {
            matches!(
                job.state,
                collaboration_client::protocol::NativeClaudeBackgroundState::Unknown { .. }
            ) && self.native_peer.is_none()
        })
    }
    pub(crate) fn with_native_inventory_view(
        mut self,
        view: crate::picker_runtime_status::NativeCatalogueView,
    ) -> Self {
        self.native_inventory_view = Some(view);
        if view == crate::picker_runtime_status::NativeCatalogueView::Stored {
            self.runtime_status = if self
                .native_background
                .as_ref()
                .is_some_and(|job| job.state.is_terminal())
            {
                PickerRuntimeStatus::NotLoaded
            } else {
                PickerRuntimeStatus::Unknown
            };
        }
        self
    }

    /// Only the qualified Stored → Loaded observation corroborates an existing row.
    /// In-stream repeats, conflicting directories and handles remain invalid.
    pub(crate) fn corroborate_native_loaded(&mut self, mut live: Self) -> Result<(), &'static str> {
        use crate::picker_runtime_status::NativeCatalogueView;
        if !matches!(self.identity, SessionPickerIdentity::NativeClaude(_))
            || self.identity != live.identity
            || self.native_inventory_view != Some(NativeCatalogueView::Stored)
            || live.native_inventory_view != Some(NativeCatalogueView::Loaded)
            || self.cwd != live.cwd
            || self.source_context != live.source_context
            || matches!((&self.native_background,&live.native_background),(Some(stored),Some(current)) if stored.job_handle!=current.job_handle)
        {
            return Err("conflicting native inventory observation");
        }
        if live.native_background.is_none() {
            live.native_background = self.native_background.clone();
        }
        live.created_at_ms = self.created_at_ms;
        live.created = self.created.clone();
        *self = live;
        Ok(())
    }

    pub(crate) fn validate_native_summary(
        summary: &ProviderSessionSummary,
        view: crate::picker_runtime_status::NativeCatalogueView,
    ) -> bool {
        use crate::picker_runtime_status::NativeCatalogueView;
        if collaboration_client::protocol::UuidIdentity::try_from(String::from(
            summary.target().session_id.clone(),
        ))
        .is_err()
        {
            return false;
        }
        match summary {
            ProviderSessionSummary::ClaudeCodeInteractive { .. } => {
                view == NativeCatalogueView::Loaded
            }
            ProviderSessionSummary::ClaudeCodeBackground { state, peer, .. } => {
                view == NativeCatalogueView::Stored || state.is_loaded() || peer.is_some()
            }
            ProviderSessionSummary::HostedProvider { .. } => false,
        }
    }

    pub(crate) fn client_provider_family(&self) -> SessionProviderFamily {
        match self.identity {
            SessionPickerIdentity::LocalCodex(_) | SessionPickerIdentity::HostedCodex(_) => {
                SessionProviderFamily::Codex
            }
            SessionPickerIdentity::HostedProvider(_) | SessionPickerIdentity::NativeClaude(_) => {
                self.provider_family
            }
        }
    }

    /// Classification uses typed, uniquely attributable endpoint evidence; it never changes inventory admission.
    pub(crate) fn with_provider_endpoint(
        mut self,
        summary: &ProviderSessionSummary,
        endpoint: &EndpointDescription,
    ) -> Self {
        if !self.identity.is_provider() {
            return self;
        }
        if let SessionPickerIdentity::NativeClaude(target) = &self.identity {
            self.provider_family = if target == summary.target()
                && target.endpoint == endpoint.endpoint
                && String::from(target.endpoint.endpoint_id.clone()) == "claude-local"
                && summary.is_native_claude()
            {
                SessionProviderFamily::External(ProviderKind::ClaudeCode)
            } else {
                SessionProviderFamily::Unclassified
            };
            return self;
        }
        let mut kinds = endpoint
            .channels
            .iter()
            .filter_map(|channel| match channel {
                ChannelDescription::ExternalProvider { runtime, .. } => Some(runtime.provider),
                _ => None,
            });
        let kind = kinds.next();
        let consistent = summary.target().endpoint == endpoint.endpoint
            && match (&self.identity, summary) {
                (
                    SessionPickerIdentity::HostedProvider(target),
                    ProviderSessionSummary::HostedProvider { .. },
                )
                | (
                    SessionPickerIdentity::NativeClaude(target),
                    ProviderSessionSummary::ClaudeCodeInteractive { .. },
                ) => target == summary.target(),
                _ => false,
            }
            && kinds.next().is_none()
            && !endpoint
                .channels
                .iter()
                .any(|channel| matches!(channel, ChannelDescription::NativeCodex { .. }))
            && !matches!(
                (summary, kind),
                (
                    ProviderSessionSummary::ClaudeCodeInteractive { .. },
                    Some(ProviderKind::Cursor)
                )
            );
        self.provider_family = match (consistent, kind) {
            (true, Some(kind)) => SessionProviderFamily::External(kind),
            _ => SessionProviderFamily::Unclassified,
        };
        self
    }

    pub(crate) fn machine_label(&self) -> &str {
        if let Some(label) = &self.machine_display_label {
            return label;
        }
        match &self.source_context {
            Some(crate::presentation::session_picker::PickerSourceContext::ConfiguredHosted(
                profile,
            )) => profile.name.as_str(),
            Some(crate::presentation::session_picker::PickerSourceContext::LocalCodex) => {
                "Local Codex"
            }
            Some(crate::presentation::session_picker::PickerSourceContext::DefaultHosted)
            | None => "This machine",
        }
    }
    pub(crate) fn matches_search(
        &self,
        expression: &collaboration_client::session_catalog::SessionSearchExpression,
    ) -> bool {
        let normalized_origin = self
            .git_origin_url
            .as_deref()
            .and_then(collaboration_client::board::normalize_git_origin_url)
            .unwrap_or_default();
        expression.matches(
            &collaboration_client::session_catalog::SessionSearchDocument {
                session_id: &self.session_id,
                name: self.explicit_name.as_deref().unwrap_or_default(),
                title: &self.full_title,
                preview: self.preview.as_deref().unwrap_or_default(),
                first_user_message: &self.first_user_message,
                branch: &self.persisted_branch,
                origin: &normalized_origin,
                cwd: self.cwd.as_deref().unwrap_or_default(),
            },
        )
    }

    pub(super) fn from_record(record: &SessionRecord) -> Self {
        Self::from_record_in_path_space(record, SessionPathSpace::InvokingMachine)
    }

    fn from_record_in_path_space(record: &SessionRecord, path_space: SessionPathSpace) -> Self {
        let display_title = display_title_from_session_fields(
            record.name.as_deref(),
            record.title.as_deref(),
            record.preview.as_deref(),
            record.first_user_message.as_deref(),
        )
        .unwrap_or_else(|| "Untitled session".to_owned());
        Self {
            identity: SessionPickerIdentity::LocalCodex(record.session_id.clone()),
            provider_family: SessionProviderFamily::Codex,
            source_context: None,
            machine_display_label: None,
            provenance: SessionRowProvenance::LocalHomeCatalog,
            endpoint_label: None,
            provider_state: None,
            native_background: None,
            native_peer: None,
            native_inventory_view: None,
            session_id: record.session_id.clone(),
            title: display_title,
            explicit_name: record.name.clone(),
            full_title: record.title.clone().unwrap_or_default(),
            recency: format_recency_at_ms(record.recency_at_ms),
            created: format_recency_at_ms(record.created_at_ms),
            recency_at_ms: record.recency_at_ms,
            created_at_ms: record.created_at_ms,
            branch: record.git_branch.clone().unwrap_or_else(|| "-".to_owned()),
            persisted_branch: record.git_branch.clone().unwrap_or_default(),
            context: record
                .cwd
                .as_deref()
                .map(session_context_from_cwd)
                .unwrap_or_else(|| "-".to_owned()),
            cwd: record.cwd.clone(),
            normalized_cwd: match path_space {
                SessionPathSpace::InvokingMachine => record.cwd.as_deref().map(|cwd| {
                    normalize_path(Path::new(cwd))
                        .to_string_lossy()
                        .into_owned()
                }),
                SessionPathSpace::SourceMachine => None,
            },
            git_origin_url: record.git_origin_url.clone(),
            provider: record.provider.clone(),
            model: record.model.clone(),
            reasoning_effort: record.reasoning_effort.clone(),
            preview: record.preview.clone(),
            first_user_message: record.first_user_message.clone().unwrap_or_default(),
            conversation: SessionConversationPreview::unavailable("history not loaded"),
            conversation_source: record.rollout_path.clone(),
            source: record.source.clone(),
            thread_source: record.thread_source.clone(),
            native_source: None,
            runtime_status: PickerRuntimeStatus::Unknown,
        }
    }

    pub(crate) fn with_hosted_codex(mut self, endpoint: &EndpointRef) -> Self {
        if let Ok(session_id) = self.session_id.clone().try_into() {
            self.provenance = SessionRowProvenance::DefaultAttributed;
            self.identity = SessionPickerIdentity::HostedCodex(SessionRef {
                endpoint: endpoint.clone(),
                session_id,
            });
        }
        self
    }

    pub(crate) fn from_provider_summary(
        summary: &ProviderSessionSummary,
        endpoint_label: &str,
    ) -> Self {
        Self::provider_summary_in_path_space(
            summary,
            endpoint_label,
            SessionPathSpace::InvokingMachine,
        )
    }

    pub(crate) fn from_source_provider_summary(
        summary: &ProviderSessionSummary,
        endpoint_label: &str,
    ) -> Self {
        Self::provider_summary_in_path_space(
            summary,
            endpoint_label,
            SessionPathSpace::SourceMachine,
        )
    }

    fn provider_summary_in_path_space(
        summary: &ProviderSessionSummary,
        endpoint_label: &str,
        path_space: SessionPathSpace,
    ) -> Self {
        let native_background = match summary {
            ProviderSessionSummary::ClaudeCodeBackground {
                job_handle,
                state,
                status,
                ..
            } => Some(NativeBackgroundRecord {
                job_handle: job_handle.clone(),
                state: state.clone(),
                status: *status,
            }),
            _ => None,
        };
        let native_peer = match summary {
            ProviderSessionSummary::ClaudeCodeBackground { peer, .. } => peer.clone(),
            ProviderSessionSummary::ClaudeCodeInteractive {
                name,
                status,
                started_at,
                updated_at,
                status_updated_at,
                kind,
                entrypoint,
                ..
            } => Some(
                collaboration_client::protocol::NativeClaudePeerObservation {
                    name: name.clone(),
                    status: *status,
                    started_at: *started_at,
                    updated_at: *updated_at,
                    status_updated_at: *status_updated_at,
                    kind: kind.clone(),
                    entrypoint: entrypoint.clone(),
                },
            ),
            ProviderSessionSummary::HostedProvider { .. } => None,
        };
        let (
            target,
            working_directory,
            updated_at_seconds,
            provider_state,
            name,
            created_at_seconds,
            runtime_status,
        ) = match summary {
            ProviderSessionSummary::HostedProvider {
                target,
                working_directory,
                updated_at,
                state,
                ..
            } => (
                target,
                working_directory,
                Some(*updated_at),
                Some(*state),
                None,
                None,
                PickerRuntimeStatus::from_provider(*state),
            ),
            ProviderSessionSummary::ClaudeCodeInteractive {
                target,
                working_directory,
                updated_at,
                started_at,
                name,
                status,
                ..
            } => (
                target,
                working_directory,
                Some(*updated_at),
                None,
                name.clone(),
                Some(*started_at),
                native_peer_runtime_status(*status),
            ),
            ProviderSessionSummary::ClaudeCodeBackground {
                target,
                working_directory,
                name,
                started_at,
                state,
                peer,
                ..
            } => (
                target,
                working_directory,
                peer.as_ref().map(|peer| peer.updated_at),
                None,
                name.clone(),
                Some(*started_at),
                NativeBackgroundRecord::runtime_status(state, peer.as_ref()),
            ),
        };
        let session_id = String::from(target.session_id.clone());
        let cwd = String::from(working_directory.clone());
        let updated_at_ms = updated_at_seconds.map(|seconds| seconds.saturating_mul(1_000));
        let name = name.and_then(|name| {
            let name = name.trim();
            (!name.is_empty()).then(|| name.to_owned())
        });
        let context = session_context_from_cwd(&cwd);
        let title = name
            .clone()
            .unwrap_or_else(|| format!("{endpoint_label} session · {context}"));
        Self {
            identity: match summary {
                ProviderSessionSummary::HostedProvider { .. } => {
                    SessionPickerIdentity::HostedProvider(target.clone())
                }
                ProviderSessionSummary::ClaudeCodeInteractive { .. }
                | ProviderSessionSummary::ClaudeCodeBackground { .. } => {
                    SessionPickerIdentity::NativeClaude(target.clone())
                }
            },
            provider_family: match summary {
                ProviderSessionSummary::ClaudeCodeInteractive { .. }
                | ProviderSessionSummary::ClaudeCodeBackground { .. } => {
                    SessionProviderFamily::External(ProviderKind::ClaudeCode)
                }
                ProviderSessionSummary::HostedProvider { .. } => {
                    SessionProviderFamily::Unclassified
                }
            },
            source_context: None,
            machine_display_label: None,
            provenance: SessionRowProvenance::ObservedProvider,
            endpoint_label: Some(endpoint_label.to_owned()),
            provider_state,
            native_background,
            native_peer,
            native_inventory_view: None,
            session_id,
            title: title.clone(),
            full_title: title,
            explicit_name: name,
            recency: format_recency_at_ms(updated_at_ms),
            created: created_at_seconds.map_or_else(
                || "-".to_owned(),
                |value| format_recency_at_ms(Some(value.saturating_mul(1_000))),
            ),
            recency_at_ms: updated_at_ms,
            created_at_ms: created_at_seconds.map(|value| value.saturating_mul(1_000)),
            branch: "-".to_owned(),
            persisted_branch: String::new(),
            context,
            cwd: Some(cwd.clone()),
            normalized_cwd: match path_space {
                SessionPathSpace::InvokingMachine => {
                    Some(normalize_path(Path::new(&cwd)).display().to_string())
                }
                SessionPathSpace::SourceMachine => None,
            },
            git_origin_url: None,
            provider: None,
            model: None,
            reasoning_effort: None,
            preview: None,
            first_user_message: String::new(),
            conversation: SessionConversationPreview::unavailable(
                "Provider history opens through its session face",
            ),
            conversation_source: None,
            source: None,
            thread_source: None,
            native_source: None,
            runtime_status,
        }
    }
}

#[path = "native_summary_projection.rs"]
mod native_summary_projection;

impl SessionConversationPreview {
    pub(crate) fn from_rollout_source(source: Option<&SessionConversationSource>) -> Self {
        let history = read_session_conversation_history(source);
        Self {
            snippets: history
                .snippets
                .into_iter()
                .map(|snippet| truncate_end(&snippet, SESSION_CONVERSATION_SNIPPET_MAX_CHARS))
                .collect(),
            unavailable_reason: history.unavailable_reason,
        }
    }

    pub(crate) fn unavailable(reason: &str) -> Self {
        Self {
            snippets: Vec::new(),
            unavailable_reason: Some(reason.to_owned()),
        }
    }
}
