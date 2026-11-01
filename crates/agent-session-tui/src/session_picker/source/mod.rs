//! source owns its picker modules and colocated checks.

#[cfg(test)]
pub(crate) mod native_catalogue_publication_test;

#[cfg(test)]
pub(crate) mod picker_machine_control_test;

pub(crate) mod picker_machine_controls;

pub(crate) mod picker_machine_selection;

#[cfg(test)]
pub(crate) mod picker_source_identity_test;

pub(crate) mod picker_source_view;

#[cfg(test)]
pub(crate) mod source_action_component_test;

pub(crate) mod source_action_state;

#[cfg(test)]
pub(crate) mod source_action_test;

pub(crate) mod source_action_worker;

pub(crate) mod source_reload_port;

pub(crate) mod source_reload_progress;

pub(crate) mod source_reload_worker;

#[cfg(test)]
pub(in crate::session_picker) use crate::session_picker::{
    PickerSourceContext, SessionsPickerOutcome,
};
