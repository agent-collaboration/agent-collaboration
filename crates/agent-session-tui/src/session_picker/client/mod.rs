//! client owns its picker modules and colocated checks.

pub(crate) mod client_action_state;

pub(crate) mod client_action_view;

#[cfg(test)]
pub(in crate::session_picker) use crate::session_picker::PickerSourceContext;
