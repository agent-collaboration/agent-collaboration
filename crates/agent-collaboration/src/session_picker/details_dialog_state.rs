//! A Details action uses its captured record, never a refreshed background focus.
use super::picker_model::SessionsPickerModel;
use crate::sessions::SessionPickerRecord;

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
    Close,
}

impl DetailsAction {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Resume => "Resume",
            Self::Load => "Load",
            Self::Fork => "Fork",
            Self::CopySessionId => "Copy session ID",
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
    pub(super) focused_action: DetailsAction,
    pub(super) clipboard_status: ClipboardRequestStatus,
}

impl SessionDetailsDialog {
    pub(super) fn capture(record: &SessionPickerRecord) -> Self {
        let target = SessionDetailsTarget::Session(Box::new(record.clone()));
        let focused_action = if record.identity.is_provider() {
            DetailsAction::CopySessionId
        } else {
            DetailsAction::Resume
        };
        Self {
            generation: 0,
            client_inventory: super::details_client_inventory::DetailsClientReadState::Unobserved,
            target,
            focused_action,
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
            _ => None,
        }
    }

    pub(super) fn actions(&self) -> &'static [DetailsAction] {
        match &self.target {
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
        }
    }

    pub(super) fn move_focus(&mut self, backwards: bool) {
        let actions = self.actions();
        let index = actions
            .iter()
            .position(|action| *action == self.focused_action)
            .unwrap_or(0);
        let next_index = if backwards {
            (index + actions.len() - 1) % actions.len()
        } else {
            (index + 1) % actions.len()
        };
        if let Some(action) = actions.get(next_index) {
            self.focused_action = *action;
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
