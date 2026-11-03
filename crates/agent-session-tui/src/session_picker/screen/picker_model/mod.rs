use crate::session_picker::filter::picker_filters::next_root_filter;
use crate::session_picker::filter::picker_filters::next_sort_filter;
use crate::session_picker::filter::picker_filters::provider_matches;
use crate::session_picker::filter::picker_filters::root_matches;
use crate::session_picker::filter::picker_filters::runtime_view_matches;
use crate::session_picker::filter::picker_filters::source_matches;
use crate::session_picker::screen::picker_actions::SessionsPickerKey;
use crate::session_picker::screen::picker_actions::SessionsPickerOutcome;
#[cfg(test)]
use crate::session_picker::screen::picker_rendering::render_model_snapshot;
use crate::session_picker::screen::picker_request::SessionsPickerRequest;
use agent_session_inventory::picker_data_query::SessionsPickerDataQuery;
use agent_session_inventory::picker_data_query::SessionsPickerRoot;
use agent_session_inventory::picker_runtime_status::PickerRecordsSnapshot;
use agent_session_inventory::picker_runtime_status::PickerRuntimeCoverage;
use agent_session_inventory::picker_runtime_status::PickerRuntimeStatus;
use agent_session_inventory::sessions::SessionPickerIdentity;
use agent_session_inventory::sessions::SessionPickerRecord;
use agent_session_inventory::sessions::SessionSearchExpression;
use agent_session_inventory::sessions::SessionsProvider;
use agent_session_inventory::sessions::SessionsSort;
use agent_session_inventory::sessions::SessionsSource;

#[cfg(test)]
pub(crate) const VISIBLE_SESSION_ROWS: usize = 8;

pub use crate::session_picker::screen::picker_model::picker_runtime_view::SessionsPickerRuntimeView;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) enum SessionsPickerFocus {
    #[default]
    StartNew,
    Session(SessionPickerIdentity),
}

/// Pure sessions picker state. iocraft owns rendering/input, this owns behavior.
#[derive(Clone, Debug)]
pub struct SessionsPickerModel {
    pub(crate) request: SessionsPickerRequest,
    pub(crate) width: usize,
    pub(crate) machine_controls:
        crate::session_picker::source::picker_machine_controls::PickerMachineControls,
    pub(crate) fork_confirmation:
        Option<crate::session_picker::fork::picker_fork_confirmation::ForkConfirmation>,
    pub(crate) source_switch_checkpoint:
        Option<crate::session_picker::source::picker_source_view::SourceSwitchCheckpoint>,
    pub(crate) source_progress:
        Vec<crate::session_picker::source::source_reload_progress::SourceReadProgress>,
    pub(crate) root: SessionsPickerRoot,
    pub(crate) provider: SessionsProvider,
    pub(crate) source: SessionsSource,
    pub runtime_view: SessionsPickerRuntimeView,
    pub(crate) sort: SessionsSort,
    pub(crate) search: String,
    pub(crate) show_help: bool,
    pub(crate) source_action:
        crate::session_picker::source::source_action_state::PickerSourceAction,
    pub(crate) client_action:
        crate::session_picker::client::client_action_state::PickerClientAction,
    pub(crate) provider_filter:
        crate::session_picker::filter::provider_filter_model::SessionProviderFilter,
    pub(crate) provider_filter_draft:
        Option<crate::session_picker::filter::provider_filter_model::ProviderFilterDraft>,
    pub(crate) new_session:
        Option<crate::session_picker::new_session::new_session_form::NewSessionDraft>,
    pub(crate) next_client_generation: u64,
    pub(crate) details_dialog:
        Option<crate::session_picker::details::details_dialog_state::SessionDetailsDialog>,
    pub(crate) runtime_coverage: PickerRuntimeCoverage,
    pub(crate) native_live_coverage:
        Vec<agent_session_inventory::picker_runtime_status::NativeLiveCoverage>,
    focus: SessionsPickerFocus,
    pointer_window_start: Option<usize>,
    visible_indices: Vec<usize>,
    visible_rows_generation: usize,
}

impl SessionsPickerModel {
    pub fn new(mut request: SessionsPickerRequest, width: usize) -> Self {
        let default_source = match request.machine_mode {
            crate::session_picker::PickerMachineSourceMode::HostedDefault => {
                crate::session_picker::PickerSourceContext::DefaultHosted
            }
            crate::session_picker::PickerMachineSourceMode::LocalCodex => {
                crate::session_picker::PickerSourceContext::LocalCodex
            }
        };
        for record in &mut request.records {
            if record.source_context.is_none() {
                record.source_context = Some(default_source.clone());
            }
        }
        let mut model = Self {
            root: request.root,
            provider: request.provider.clone(),
            source: request.source,
            runtime_view: SessionsPickerRuntimeView::All,
            sort: request.sort,
            request,
            width,
            machine_controls: crate::session_picker::source::picker_machine_controls::PickerMachineControls::default(),
            fork_confirmation: None,
            source_progress: Vec::new(),
            source_switch_checkpoint: None,
            search: String::new(),
            show_help: false,
            source_action: crate::session_picker::source::source_action_state::PickerSourceAction::default(),
            client_action: crate::session_picker::client::client_action_state::PickerClientAction::Idle,
            provider_filter: crate::session_picker::filter::provider_filter_model::SessionProviderFilter::All,
            provider_filter_draft: None,
            details_dialog: None,
            new_session: None,
            next_client_generation: 0,
            runtime_coverage: PickerRuntimeCoverage::Unobserved,
            native_live_coverage: Vec::new(),
            focus: SessionsPickerFocus::StartNew,
            pointer_window_start: None,
            visible_indices: Vec::new(),
            visible_rows_generation: 0,
        };
        model.rebuild_visible_rows();
        model.focus_first_record_when_available();
        model
    }

    pub fn handle_key(&mut self, key: SessionsPickerKey) {
        #[cfg(test)]
        let help_key = matches!(key, SessionsPickerKey::ToggleHelp);
        #[cfg(not(test))]
        let help_key = false;
        if self.source_action.pending.is_some() && !help_key {
            return;
        }
        if !help_key {
            self.pointer_window_start = None;
        }
        match key {
            #[cfg(test)]
            SessionsPickerKey::MoveDown => {
                let visible_len = self.visible_len();
                self.focus_visible_index(
                    (self.focused_visible_index() + 1).min(visible_len.saturating_sub(1)),
                );
            }
            #[cfg(test)]
            SessionsPickerKey::MoveUp => {
                self.focus_visible_index(self.focused_visible_index().saturating_sub(1));
            }
            #[cfg(test)]
            SessionsPickerKey::PageDown => {
                let visible_len = self.visible_len();
                self.focus_visible_index(
                    (self.focused_visible_index() + VISIBLE_SESSION_ROWS)
                        .min(visible_len.saturating_sub(1)),
                );
            }
            #[cfg(test)]
            SessionsPickerKey::PageUp => {
                self.focus_visible_index(
                    self.focused_visible_index()
                        .saturating_sub(VISIBLE_SESSION_ROWS),
                );
            }
            #[cfg(test)]
            SessionsPickerKey::MoveFirst => {
                self.focus_start_new();
            }
            #[cfg(test)]
            SessionsPickerKey::MoveLast => {
                let visible_len = self.visible_len();
                self.focus_visible_index(visible_len.saturating_sub(1));
            }
            SessionsPickerKey::CycleRoot => {
                let previous_index = self.focused_visible_index();
                self.root = next_root_filter(self.root);
                self.rebuild_visible_rows();
                self.restore_focus_or_fallback(previous_index);
            }
            SessionsPickerKey::CycleRuntimeView => {
                let previous_index = self.focused_visible_index();
                self.runtime_view = self.runtime_view.next();
                self.rebuild_visible_rows();
                self.restore_focus_or_fallback(previous_index);
            }
            SessionsPickerKey::CycleSort => {
                let previous_index = self.focused_visible_index();
                self.sort = next_sort_filter(self.sort);
                self.rebuild_visible_rows();
                self.restore_focus_or_fallback(previous_index);
            }
            #[cfg(test)]
            SessionsPickerKey::ToggleHelp => {
                self.show_help = !self.show_help;
            }
            #[cfg(test)]
            SessionsPickerKey::SearchChar(character) => {
                let previous_index = self.focused_visible_index();
                if !character.is_control() {
                    self.search.push(character);
                    self.rebuild_visible_rows();
                }
                self.restore_focus_or_fallback(previous_index);
            }
            #[cfg(test)]
            SessionsPickerKey::SearchBackspace => {
                let previous_index = self.focused_visible_index();
                if self.search.pop().is_some() {
                    self.rebuild_visible_rows();
                    self.restore_focus_or_fallback(previous_index);
                }
            }
            SessionsPickerKey::ClearSearch => {
                let previous_index = self.focused_visible_index();
                if !self.search.is_empty() {
                    self.search.clear();
                    self.rebuild_visible_rows();
                    self.restore_focus_or_fallback(previous_index);
                }
            }
        }
    }

    pub(crate) fn append_search_text(&mut self, text: &str) {
        if self.source_action.pending.is_some() {
            return;
        }
        let previous_length = self.search.len();
        self.search
            .extend(text.chars().filter(|character| !character.is_control()));
        if self.search.len() == previous_length {
            return;
        }
        let previous_index = self.focused_visible_index();
        self.pointer_window_start = None;
        self.rebuild_visible_rows();
        self.restore_focus_or_fallback(previous_index);
    }

    pub fn set_width(&mut self, width: usize) {
        self.width = width;
    }

    pub(crate) fn rebuild_browse_choices(&mut self) {
        let previous_index = self.focused_visible_index();
        self.pointer_window_start = None;
        self.rebuild_visible_rows();
        self.restore_focus_or_fallback(previous_index);
    }

    pub(crate) fn open_provider_filters(&mut self) {
        if self.fork_confirmation.is_none()
            && matches!(
                self.machine_controls.stage,
                crate::session_picker::source::picker_machine_controls::PickerMachineStage::Browsing
            )
            && self.provider_filter_draft.is_none()
            && self.details_dialog.is_none()
        {
            self.provider_filter_draft = Some(
                crate::session_picker::filter::provider_filter_model::ProviderFilterDraft::new(
                    self.provider_filter,
                    self.data_query(),
                    self.runtime_view,
                ),
            );
        }
    }

    pub(crate) fn apply_provider_filters(&mut self) {
        let Some(draft) = self.provider_filter_draft.take() else {
            return;
        };
        self.provider_filter = draft.selection;
        self.root = draft.query.root;
        self.provider = draft.query.provider;
        self.source = draft.query.source;
        self.sort = draft.query.sort;
        self.runtime_view = draft.runtime_view;
        self.pointer_window_start = None;
        self.rebuild_visible_rows();
        if self
            .focused_identity()
            .is_some_and(|identity| self.visible_index_for_identity(identity).is_none())
        {
            self.focus_start_new();
            self.focus_first_record_when_available();
        }
    }

    pub(crate) fn open_details_dialog(&mut self) {
        if self.focused_record().is_none() {
            self.open_new_form();
            return;
        }
        if self.provider_filter_draft.is_none()
            && self.fork_confirmation.is_none()
            && matches!(
                self.machine_controls.stage,
                crate::session_picker::source::picker_machine_controls::PickerMachineStage::Browsing
            )
        {
            self.open_captured_details();
        }
    }

    pub fn data_query(&self) -> SessionsPickerDataQuery {
        SessionsPickerDataQuery {
            root: self.root,
            provider: self.provider.clone(),
            source: self.source,
            sort: self.sort,
            search: self.search.clone(),
            include_empty_sessions: self.request.include_empty_sessions,
        }
    }

    pub fn replace_records(&mut self, snapshot: PickerRecordsSnapshot) {
        let previous_index = self.focused_visible_index();
        self.pointer_window_start = None;
        self.request.records = snapshot.records;
        let default_source = self.source_contexts().into_iter().next();
        for record in &mut self.request.records {
            if record.source_context.is_none() {
                record.source_context = default_source.clone();
            }
        }
        self.runtime_coverage = snapshot.runtime_coverage;
        self.native_live_coverage = snapshot.native_live_coverage;
        self.rebuild_visible_rows();
        self.restore_focus_or_fallback(previous_index);
    }

    pub fn invalidate_runtime_statuses(&mut self) {
        self.runtime_coverage = PickerRuntimeCoverage::Unavailable;
        self.native_live_coverage.clear();
        if self.request.records.iter().all(|record| {
            record.runtime_status == PickerRuntimeStatus::Unknown && record.provider_state.is_none()
        }) {
            return;
        }
        let previous_index = self.focused_visible_index();
        for record in &mut self.request.records {
            record.runtime_status = PickerRuntimeStatus::Unknown;
            record.provider_state = None;
            record.native_peer = None;
        }
        self.rebuild_visible_rows();
        self.restore_focus_or_fallback(previous_index);
    }

    #[cfg(test)]
    pub fn focus_visible_session(&mut self, session_id: &str) -> bool {
        self.pointer_window_start = None;
        let identity = self
            .request
            .records
            .iter()
            .find(|record| record.session_id == session_id)
            .map(|record| record.identity.clone());
        identity.is_some_and(|identity| self.focus_visible_identity_in_window(&identity, None))
    }

    #[cfg(test)]
    pub fn focus_visible_session_in_window(
        &mut self,
        session_id: &str,
        window_start: Option<usize>,
    ) -> bool {
        let identity = self
            .request
            .records
            .iter()
            .find(|record| record.session_id == session_id)
            .map(|record| record.identity.clone());
        identity
            .is_some_and(|identity| self.focus_visible_identity_in_window(&identity, window_start))
    }

    #[cfg(test)]
    pub fn focus_visible_identity(&mut self, identity: &SessionPickerIdentity) -> bool {
        self.focus_visible_identity_in_window(identity, None)
    }

    pub fn focus_visible_identity_in_window(
        &mut self,
        identity: &SessionPickerIdentity,
        window_start: Option<usize>,
    ) -> bool {
        if self.provider_filter_draft.is_some() || self.details_dialog.is_some() {
            return false;
        }
        if self.visible_index_for_identity(identity).is_none() {
            return false;
        }
        self.pointer_window_start = window_start;
        if self.focused_identity() != Some(identity) {
            self.focus = SessionsPickerFocus::Session(identity.clone());
        }
        true
    }

    pub fn focus_start_new(&mut self) {
        self.pointer_window_start = None;
        self.focus = SessionsPickerFocus::StartNew;
    }

    #[cfg(test)]
    pub fn focused_session_id(&self) -> Option<&str> {
        self.focused_record()
            .map(|record| record.session_id.as_str())
    }

    pub fn focused_identity(&self) -> Option<&SessionPickerIdentity> {
        match &self.focus {
            SessionsPickerFocus::StartNew => None,
            SessionsPickerFocus::Session(identity) => Some(identity),
        }
    }

    pub(crate) fn source_contexts(&self) -> Vec<crate::session_picker::PickerSourceContext> {
        if let crate::session_picker::source::picker_machine_controls::PickerMachineStage::Loading { source, .. } =
            &self.machine_controls.stage
        {
            return vec![source.as_ref().clone()];
        }
        if let crate::session_picker::source::picker_machine_controls::PickerMachineFilter::Single { source } =
            &self.machine_controls.filter
        {
            return vec![source.as_ref().clone()];
        }
        let default_source = match self.request.machine_mode {
            crate::session_picker::PickerMachineSourceMode::HostedDefault => {
                crate::session_picker::PickerSourceContext::DefaultHosted
            }
            crate::session_picker::PickerMachineSourceMode::LocalCodex => {
                crate::session_picker::PickerSourceContext::LocalCodex
            }
        };
        let mut sources = vec![default_source];
        if self.machine_controls.filter
            == crate::session_picker::source::picker_machine_controls::PickerMachineFilter::All
            && let agent_session_inventory::sessions::RouterRegistryRead::Ready(registry) =
                &self.request.router_registry
        {
            sources.extend(
                registry
                    .routers
                    .iter()
                    .cloned()
                    .map(crate::session_picker::PickerSourceContext::ConfiguredHosted),
            );
        }
        sources
    }

    pub(crate) fn start_new_action(&mut self) -> Option<SessionsPickerOutcome> {
        if self.provider_filter_draft.is_some() || self.details_dialog.is_some() {
            return None;
        }
        self.open_new_form();
        None
    }

    pub fn activation_outcome_for_focus(&self) -> Option<SessionsPickerOutcome> {
        match self.focused_record() {
            Some(record) if record.identity.is_provider() => None,
            Some(record) => Some(SessionsPickerOutcome::ResumeSession(
                agent_session_inventory::sessions::SessionActionSelection::from_picker_record(
                    record,
                ),
            )),
            None => Some(SessionsPickerOutcome::StartNewSession),
        }
    }

    #[cfg(test)]
    pub fn fork_outcome_for_focus(&self) -> Option<SessionsPickerOutcome> {
        self.focused_record()
            .filter(|record| !record.identity.is_provider())
            .map(|record| {
                SessionsPickerOutcome::ForkSession(
                    agent_session_inventory::sessions::SessionActionSelection::from_picker_record(
                        record,
                    ),
                )
            })
    }

    #[cfg(test)]
    pub fn render_snapshot(&self) -> String {
        render_model_snapshot(self)
    }

    pub(crate) fn open_fork_confirmation(&mut self) {
        self.fork_confirmation = self.focused_record().map(|record| {
            crate::session_picker::fork::picker_fork_confirmation::ForkConfirmation::capture(
                record,
                &self.request,
            )
        });
    }

    #[cfg(test)]
    pub fn visible_rows_generation(&self) -> usize {
        self.visible_rows_generation
    }

    pub(crate) fn visible_len(&self) -> usize {
        self.visible_indices.len() + 1
    }

    pub(crate) fn visible_record_len(&self) -> usize {
        self.visible_indices.len()
    }

    pub(crate) fn focused_visible_index(&self) -> usize {
        match self.focused_identity() {
            Some(identity) => self.visible_index_for_identity(identity).unwrap_or(0),
            None => 0,
        }
    }

    pub(crate) fn focused_record(&self) -> Option<&SessionPickerRecord> {
        self.visible_choice_record_at(self.focused_visible_index())
    }

    #[cfg(test)]
    pub(crate) fn focused_window_start(&self, visible_rows: usize) -> usize {
        let visible_len = self.visible_len();
        let maximum_window_start = visible_len.saturating_sub(visible_rows);
        if let Some(pointer_window_start) = self.pointer_window_start {
            let pointer_window_start = pointer_window_start.min(maximum_window_start);
            let focused_index = self.focused_visible_index();
            if focused_index >= pointer_window_start
                && focused_index < pointer_window_start.saturating_add(visible_rows)
            {
                return pointer_window_start;
            }
        }
        visible_window_start(self.focused_visible_index(), visible_len, visible_rows)
    }

    pub(crate) fn visible_choice_record_at(&self, index: usize) -> Option<&SessionPickerRecord> {
        if index == 0 {
            return None;
        }
        self.visible_record_at(index - 1)
    }

    pub(crate) fn visible_record_at(&self, index: usize) -> Option<&SessionPickerRecord> {
        self.visible_indices
            .get(index)
            .and_then(|record_index| self.request.records.get(*record_index))
    }

    pub fn session_groups<TZone: chrono::TimeZone>(
        &self,
        facet: crate::session_picker::SessionGroupFacet,
        now: chrono::DateTime<chrono::Utc>,
        time_zone: &TZone,
    ) -> Vec<crate::session_picker::SessionGroup> {
        crate::session_picker::compute_session_groups(
            facet,
            self.visible_indices
                .iter()
                .filter_map(|index| self.request.records.get(*index)),
            now,
            time_zone,
            &self.source_contexts(),
        )
    }

    fn rebuild_visible_rows(&mut self) {
        let search = SessionSearchExpression::parse(&self.search);
        let mut indices = self
            .request
            .records
            .iter()
            .enumerate()
            .filter(|(_index, record)| root_matches(self.root, &self.request, record))
            .filter(|(_index, record)| provider_matches(&self.provider, &self.request, record))
            .filter(|(_index, record)| source_matches(self.source, record))
            .filter(|(_index, record)| runtime_view_matches(self.runtime_view, record))
            .filter(|(_index, record)| {
                self.provider_filter
                    .matches(record.client_provider_family())
            })
            .filter(|(_index, record)| search.is_empty() || record.matches_search(&search))
            .map(|(index, _record)| index)
            .collect::<Vec<_>>();
        indices.sort_by(|left_index, right_index| {
            let Some(left) = self.request.records.get(*left_index) else {
                return left_index.cmp(right_index);
            };
            let Some(right) = self.request.records.get(*right_index) else {
                return left_index.cmp(right_index);
            };
            match self.sort {
                SessionsSort::Updated => right
                    .recency_at_ms
                    .unwrap_or(i64::MIN)
                    .cmp(&left.recency_at_ms.unwrap_or(i64::MIN)),
                SessionsSort::Created => right
                    .created_at_ms
                    .unwrap_or(i64::MIN)
                    .cmp(&left.created_at_ms.unwrap_or(i64::MIN)),
            }
        });
        self.visible_indices = indices;
        self.visible_rows_generation = self.visible_rows_generation.saturating_add(1);
    }

    fn focus_first_record_when_available(&mut self) {
        if let Some(identity) = self
            .visible_record_at(0)
            .map(|record| record.identity.clone())
        {
            self.focus = SessionsPickerFocus::Session(identity);
        }
    }

    fn focus_visible_index(&mut self, index: usize) {
        if index == 0 {
            self.focus_start_new();
            return;
        }
        if let Some(identity) = self
            .visible_choice_record_at(index)
            .map(|record| record.identity.clone())
        {
            self.focus = SessionsPickerFocus::Session(identity);
        }
    }

    fn visible_index_for_identity(&self, identity: &SessionPickerIdentity) -> Option<usize> {
        self.visible_indices
            .iter()
            .position(|record_index| {
                self.request
                    .records
                    .get(*record_index)
                    .is_some_and(|record| &record.identity == identity)
            })
            .map(|index| index + 1)
    }

    fn restore_focus_or_fallback(&mut self, previous_index: usize) {
        if self
            .focused_identity()
            .is_some_and(|identity| self.visible_index_for_identity(identity).is_some())
        {
            return;
        }
        self.focus_visible_index(previous_index.min(self.visible_len().saturating_sub(1)));
    }
}

#[cfg(test)]
pub(in crate::session_picker::screen) use crate::session_picker::screen::picker_model::picker_visible_window::visible_window_start;

pub(crate) mod picker_runtime_view;

pub(crate) mod picker_visible_window;
