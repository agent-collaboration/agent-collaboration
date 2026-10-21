//! Native job lifecycle and peer runtime are independent observed facts.
use crate::picker_runtime_status::PickerRuntimeStatus;
use collaboration_client::protocol::ClaudeCodeInteractiveStatus;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NativeBackgroundRecord {
    pub(crate) job_handle: collaboration_client::protocol::NativeClaudeJobHandle,
    pub(crate) state: collaboration_client::protocol::NativeClaudeBackgroundState,
    pub(crate) status: Option<ClaudeCodeInteractiveStatus>,
}
impl NativeBackgroundRecord {
    pub(super) fn runtime_status(
        state: &collaboration_client::protocol::NativeClaudeBackgroundState,
        peer: Option<&collaboration_client::protocol::NativeClaudePeerObservation>,
    ) -> PickerRuntimeStatus {
        if state.is_active() {
            return PickerRuntimeStatus::Active;
        }
        if let Some(peer) = peer {
            return native_peer_runtime_status(peer.status);
        }
        use collaboration_client::protocol::NativeClaudeBackgroundState;
        match state {
            NativeClaudeBackgroundState::Working => PickerRuntimeStatus::Active,
            NativeClaudeBackgroundState::Blocked => PickerRuntimeStatus::Blocked,
            NativeClaudeBackgroundState::Done
            | NativeClaudeBackgroundState::Failed
            | NativeClaudeBackgroundState::Stopped => PickerRuntimeStatus::NotLoaded,
            NativeClaudeBackgroundState::Unknown { .. } => PickerRuntimeStatus::Unknown,
        }
    }
}
pub(super) fn native_peer_runtime_status(
    status: ClaudeCodeInteractiveStatus,
) -> PickerRuntimeStatus {
    match status {
        ClaudeCodeInteractiveStatus::Busy => PickerRuntimeStatus::Active,
        ClaudeCodeInteractiveStatus::Idle | ClaudeCodeInteractiveStatus::Shell => {
            PickerRuntimeStatus::Idle
        }
        ClaudeCodeInteractiveStatus::Waiting => PickerRuntimeStatus::Blocked,
        ClaudeCodeInteractiveStatus::Unreported | ClaudeCodeInteractiveStatus::Other => {
            PickerRuntimeStatus::Unknown
        }
    }
}
