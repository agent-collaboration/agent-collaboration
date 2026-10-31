//! Client-provider visibility is transient presentation state, separate from backend queries.
use agent_session_inventory::session_provider_family::SessionProviderFamily;
use collaboration_client::protocol::ProviderKind;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum SessionProviderFilter {
    #[default]
    All,
    Codex,
    ClaudeCode,
    Cursor,
}

impl SessionProviderFilter {
    pub(super) const CHOICES: [Self; 4] = [Self::All, Self::Codex, Self::ClaudeCode, Self::Cursor];

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::All => "All providers",
            Self::Codex => "Codex",
            Self::ClaudeCode => "Claude Code",
            Self::Cursor => "Cursor",
        }
    }

    pub(super) fn matches(self, family: SessionProviderFamily) -> bool {
        match self {
            Self::All => true,
            Self::Codex => family == SessionProviderFamily::Codex,
            Self::ClaudeCode => family == SessionProviderFamily::External(ProviderKind::ClaudeCode),
            Self::Cursor => family == SessionProviderFamily::External(ProviderKind::Cursor),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum ProviderFilterFocus {
    #[default]
    Provider,
    Folder,
    ModelProvider,
    Source,
    Runtime,
    Sort,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ProviderFilterDraft {
    pub(super) selection: SessionProviderFilter,
    pub(super) query: super::SessionsPickerDataQuery,
    pub(super) runtime_view: super::picker_model::SessionsPickerRuntimeView,
}

impl ProviderFilterDraft {
    pub(super) fn new(
        selection: SessionProviderFilter,
        query: super::SessionsPickerDataQuery,
        runtime_view: super::picker_model::SessionsPickerRuntimeView,
    ) -> Self {
        Self {
            selection,
            query,
            runtime_view,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum SessionFilterValue {
    Client(SessionProviderFilter),
    Folder(super::SessionsPickerRoot),
    ModelProvider(agent_session_inventory::sessions::SessionsProvider),
    Source(agent_session_inventory::sessions::SessionsSource),
    Runtime(super::picker_model::SessionsPickerRuntimeView),
    Sort(agent_session_inventory::sessions::SessionsSort),
    CheckoutUnavailable,
}

impl ProviderFilterDraft {
    pub(super) fn field_value(&self, field: ProviderFilterFocus) -> Option<SessionFilterValue> {
        Some(match field {
            ProviderFilterFocus::Provider => SessionFilterValue::Client(self.selection),
            ProviderFilterFocus::Folder => SessionFilterValue::Folder(self.query.root),
            ProviderFilterFocus::ModelProvider => {
                SessionFilterValue::ModelProvider(self.query.provider.clone())
            }
            ProviderFilterFocus::Source => SessionFilterValue::Source(self.query.source),
            ProviderFilterFocus::Runtime => SessionFilterValue::Runtime(self.runtime_view),
            ProviderFilterFocus::Sort => SessionFilterValue::Sort(self.query.sort),
        })
    }
    pub(super) fn select_value(&mut self, value: SessionFilterValue) {
        match value {
            SessionFilterValue::Client(value) => self.selection = value,
            SessionFilterValue::Folder(value) => self.query.root = value,
            SessionFilterValue::ModelProvider(value) => self.query.provider = value,
            SessionFilterValue::Source(value) => self.query.source = value,
            SessionFilterValue::Runtime(value) => self.runtime_view = value,
            SessionFilterValue::Sort(value) => self.query.sort = value,
            SessionFilterValue::CheckoutUnavailable => {}
        }
    }
}

pub(super) fn filter_choices(
    model: &super::picker_model::SessionsPickerModel,
    field: ProviderFilterFocus,
) -> Vec<agent_collaboration_tui::ChoiceOption<SessionFilterValue>> {
    use super::picker_model::SessionsPickerRuntimeView;
    use super::{PickerSourceContext, SessionsPickerRoot};
    use agent_collaboration_tui::{ChoiceAvailability, ChoiceOption};
    use agent_session_inventory::sessions::{SessionsProvider, SessionsSort, SessionsSource};
    let configured = model
        .source_contexts()
        .iter()
        .any(|source| matches!(source, PickerSourceContext::ConfiguredHosted(_)));
    let external = model
        .request
        .records
        .iter()
        .any(|record| record.identity.is_provider());
    let mut choices = match field {
        ProviderFilterFocus::Provider => SessionProviderFilter::CHOICES
            .into_iter()
            .map(|value| ChoiceOption::available(SessionFilterValue::Client(value), value.label()))
            .collect(),
        ProviderFilterFocus::Folder => vec![
            ChoiceOption::available(
                SessionFilterValue::Folder(SessionsPickerRoot::Cwd),
                "Current directory",
            ),
            ChoiceOption {
                key: SessionFilterValue::CheckoutUnavailable,
                label: "Checkout".to_owned(),
                secondary: None,
                availability: ChoiceAvailability::Unavailable {
                    reason: "Checkout scope is supported by CLI --checkout --list only".to_owned(),
                },
            },
            ChoiceOption::available(
                SessionFilterValue::Folder(SessionsPickerRoot::Repo),
                "Repository / linked worktrees",
            ),
            ChoiceOption::available(
                SessionFilterValue::Folder(SessionsPickerRoot::Any),
                "Any folder",
            ),
        ],
        ProviderFilterFocus::ModelProvider => {
            let mut options = vec![
                ChoiceOption::available(
                    SessionFilterValue::ModelProvider(SessionsProvider::Any),
                    "Any model provider",
                ),
                ChoiceOption::available(
                    SessionFilterValue::ModelProvider(SessionsProvider::Current),
                    "Current model provider",
                ),
            ];
            let mut ids = model
                .request
                .records
                .iter()
                .filter_map(|record| record.provider.clone())
                .collect::<Vec<_>>();
            if let SessionsProvider::Id(id) = &model.provider {
                ids.push(id.clone());
            }
            ids.sort();
            ids.dedup();
            options.extend(ids.into_iter().map(|id| {
                ChoiceOption::available(
                    SessionFilterValue::ModelProvider(SessionsProvider::Id(id.clone())),
                    id,
                )
            }));
            options
        }
        ProviderFilterFocus::Source => [
            SessionsSource::Interactive,
            SessionsSource::Subagents,
            SessionsSource::All,
        ]
        .into_iter()
        .map(|source| {
            ChoiceOption::available(
                SessionFilterValue::Source(source),
                match source {
                    SessionsSource::Interactive => "Interactive",
                    SessionsSource::Subagents => "Subagents",
                    SessionsSource::All => "All sources",
                },
            )
        })
        .collect(),
        ProviderFilterFocus::Runtime => [
            SessionsPickerRuntimeView::All,
            SessionsPickerRuntimeView::Active,
            SessionsPickerRuntimeView::Idle,
            SessionsPickerRuntimeView::Blocked,
        ]
        .into_iter()
        .map(|view| ChoiceOption::available(SessionFilterValue::Runtime(view), format!("{view:?}")))
        .collect(),
        ProviderFilterFocus::Sort => vec![
            ChoiceOption::available(SessionFilterValue::Sort(SessionsSort::Updated), "Updated"),
            ChoiceOption::available(SessionFilterValue::Sort(SessionsSort::Created), "Created"),
        ],
    };
    for choice in &mut choices {
        let reason = match &choice.key {
            SessionFilterValue::Folder(root) if configured && *root != SessionsPickerRoot::Any => {
                Some("Configured machines require Any folder; current query is retained")
            }
            SessionFilterValue::ModelProvider(provider)
                if configured && *provider != SessionsProvider::Any =>
            {
                Some(
                    "Configured machines require Any model provider; client filtering remains available",
                )
            }
            SessionFilterValue::Sort(sort) if configured && *sort != SessionsSort::Updated => {
                Some("Configured machines support Updated sort only")
            }
            SessionFilterValue::Source(source) if external && *source != SessionsSource::All => {
                Some("External provider inventories require All sources")
            }
            _ => None,
        };
        if let Some(reason) = reason {
            choice.availability = ChoiceAvailability::Unavailable {
                reason: reason.to_owned(),
            };
        }
    }
    choices
}
