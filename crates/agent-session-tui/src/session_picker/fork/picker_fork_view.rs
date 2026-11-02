//! Fork content and callbacks reuse shared chrome; the captured selection owns the effect.
use crate::session_picker::SessionsPickerOutcome;
use crate::session_picker::fork::picker_fork_confirmation::ForkLaunchTarget;
use crate::session_picker::screen::picker_model::SessionsPickerModel;
use iocraft::prelude::*;

pub(crate) fn confirm_fork(
    mut state: State<SessionsPickerModel>,
    mut outcome: State<Option<SessionsPickerOutcome>>,
) {
    let mut model = state.write();
    if model.show_help || model.client_action.is_active() || model.source_action.pending.is_some() {
        return;
    }
    if !model.begin_source_fork()
        && let Some(confirmation) = model.fork_confirmation.as_mut()
    {
        match confirmation.confirm() {
            Some(ForkLaunchTarget::Codex(selection)) => {
                outcome.set(Some(SessionsPickerOutcome::ForkSession(*selection)))
            }
            Some(ForkLaunchTarget::Native(selection)) => {
                model.begin_native_fork(*selection);
            }
            Some(ForkLaunchTarget::Unsupported) | None => {}
        }
    }
}
