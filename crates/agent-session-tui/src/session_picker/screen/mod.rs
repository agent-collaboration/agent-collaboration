//! screen owns its picker modules and colocated checks.

#[cfg(test)]
pub(crate) mod form_alignment_test;

pub(crate) mod picker_actions;

#[cfg(test)]
pub(crate) mod picker_background_test;

#[cfg(test)]
pub(crate) mod picker_capture_test;

pub(crate) mod picker_choice_scope;

pub(crate) mod picker_choice;

pub(crate) mod picker_component;

pub(crate) mod picker_dialog_scope;

pub(crate) mod picker_display_text;

pub(crate) mod picker_entrypoint;

pub(crate) mod picker_help_content;

#[cfg(test)]
pub(crate) mod picker_help_test;

#[cfg(test)]
pub(crate) mod picker_interaction_test;

#[cfg(test)]
pub(crate) mod picker_layout_budget;

#[cfg(test)]
pub(crate) mod picker_layout_test;

pub(crate) mod picker_list_view;

#[cfg(test)]
pub(crate) mod picker_menu_section_test;

#[cfg(test)]
pub(crate) mod picker_model_test;

pub(crate) mod picker_model;

#[cfg(test)]
pub(crate) mod picker_pane_stability_test;

#[cfg(test)]
pub(crate) mod picker_performance_test;

#[cfg(test)]
pub(crate) mod picker_pointer_test;

#[cfg(test)]
pub(crate) mod picker_r15_contract_test;

pub(crate) mod picker_rendering;

pub(crate) mod picker_request;

#[cfg(test)]
pub(crate) mod picker_scope_contract_test;

pub(crate) mod picker_screen_scope;

#[cfg(test)]
pub(crate) mod picker_single_view_test;

pub(crate) mod picker_surface_view;

pub(crate) mod picker_surface;

#[cfg(test)]
pub(crate) mod picker_taste_review_test;

#[cfg(test)]
pub(crate) mod session_choice_ux_test;

#[cfg(test)]
pub(crate) mod session_context_ux_test;

#[cfg(test)]
pub(crate) use picker_model::SessionsPickerRuntimeView;

#[cfg(any(test, feature = "quota-reset-test-harness"))]
pub(crate) mod fixtures;
