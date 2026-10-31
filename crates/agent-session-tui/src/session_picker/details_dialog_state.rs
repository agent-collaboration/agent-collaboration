//! A Details action uses its captured record, never a refreshed background focus.
use super::picker_model::SessionsPickerModel;
use agent_session_inventory::sessions::SessionPickerRecord;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum SessionDetailsTarget {
    Session(Box<SessionPickerRecord>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DetailsAction {
    Resume,
    Load,
    Fork,
    CopySessionId,
    CopyPath,
    Close,
}

impl DetailsAction {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Resume => "Resume",
            Self::Load => "Load",
            Self::Fork => "Fork",
            Self::CopySessionId => "Copy session ID",
            Self::CopyPath => "Copy path",
            Self::Close => "Close",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum ClipboardRequestStatus {
    #[default]
    Idle,
    Sent,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct SessionDetailsDialog {
    pub(super) target: SessionDetailsTarget,
    pub(super) generation: u64,
    pub(super) client_inventory: super::details_client_inventory::DetailsClientReadState,
    pub(super) clipboard_status: ClipboardRequestStatus,
}

impl SessionDetailsDialog {
    pub(super) fn capture(record: &SessionPickerRecord) -> Self {
        let target = SessionDetailsTarget::Session(Box::new(record.clone()));
        Self {
            generation: 0,
            client_inventory: super::details_client_inventory::DetailsClientReadState::Unobserved,
            target,
            clipboard_status: ClipboardRequestStatus::Idle,
        }
    }

    pub(super) fn record(&self) -> &SessionPickerRecord {
        match &self.target {
            SessionDetailsTarget::Session(record) => record,
        }
    }

    pub(super) fn inspection_notice(&self) -> Option<&str> {
        use super::details_client_inventory::{DetailsClientCapabilities, DetailsClientReadState};
        if !self.record().identity.is_provider() {
            return None;
        }
        match &self.client_inventory {
            DetailsClientReadState::Unobserved => Some("Client actions have not been checked"),
            DetailsClientReadState::Checking => Some("Checking source session actions"),
            DetailsClientReadState::Unavailable(reason)
            | DetailsClientReadState::Ready(DetailsClientCapabilities::Unavailable(reason)) => {
                Some(reason)
            }
            DetailsClientReadState::Ready(_) => None,
        }
    }

    pub(super) fn actions(&self) -> Vec<DetailsAction> {
        let actions: &[DetailsAction] = match &self.target {
            SessionDetailsTarget::Session(record) if record.identity.is_provider() => {
                use super::details_client_inventory::{
                    DetailsClientCapabilities, DetailsClientReadState, ManagedRestoreActions,
                };
                match &self.client_inventory {
                    DetailsClientReadState::Ready(DetailsClientCapabilities::Native { .. }) => &[
                        DetailsAction::Close,
                        DetailsAction::CopySessionId,
                        DetailsAction::Fork,
                        DetailsAction::Resume,
                    ],
                    DetailsClientReadState::Ready(DetailsClientCapabilities::Managed {
                        actions,
                        ..
                    }) => match actions {
                        ManagedRestoreActions::Load => &[
                            DetailsAction::Close,
                            DetailsAction::CopySessionId,
                            DetailsAction::Load,
                        ],
                        ManagedRestoreActions::Resume => &[
                            DetailsAction::Close,
                            DetailsAction::CopySessionId,
                            DetailsAction::Resume,
                        ],
                        ManagedRestoreActions::LoadAndResume => &[
                            DetailsAction::Close,
                            DetailsAction::CopySessionId,
                            DetailsAction::Load,
                            DetailsAction::Resume,
                        ],
                    },
                    _ => &[DetailsAction::Close, DetailsAction::CopySessionId],
                }
            }
            SessionDetailsTarget::Session(_) => &[
                DetailsAction::Close,
                DetailsAction::CopySessionId,
                DetailsAction::Fork,
                DetailsAction::Resume,
            ],
        };
        let mut actions = actions.to_vec();
        if self
            .record()
            .cwd
            .as_deref()
            .is_some_and(|path| !path.trim().is_empty())
        {
            let position = actions
                .iter()
                .position(|action| *action == DetailsAction::CopySessionId)
                .map_or(actions.len(), |index| index + 1);
            actions.insert(position, DetailsAction::CopyPath);
        }
        actions
    }

    pub(super) fn action_label(&self, action: DetailsAction) -> &'static str {
        if action == DetailsAction::Resume
            && matches!(
                self.record().identity,
                agent_session_inventory::sessions::SessionPickerIdentity::NativeClaude(_)
            )
        {
            "Open"
        } else {
            action.label()
        }
    }
}

impl SessionsPickerModel {
    pub(super) fn open_captured_details(&mut self) {
        let Some(record) = self.focused_record().cloned() else {
            return;
        };
        let Some(generation) = self.next_client_generation.checked_add(1) else {
            return;
        };
        self.next_client_generation = generation;
        let mut dialog = SessionDetailsDialog::capture(&record);
        dialog.generation = generation;
        self.details_dialog = Some(dialog);
    }
}
