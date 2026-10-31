#[cfg(test)]
#[path = "suites/action_controls.rs"]
mod action_controls;
#[cfg(test)]
#[path = "suites/button_focus_style.rs"]
mod button_focus_style;
#[cfg(test)]
#[path = "suites/combo_controls.rs"]
mod combo_controls;
#[cfg(test)]
#[path = "suites/combo_filter_layout.rs"]
mod combo_filter_layout;
#[cfg(test)]
#[path = "suites/control_editor.rs"]
mod control_editor;
#[cfg(test)]
#[path = "suites/dialog_body_restore.rs"]
mod dialog_body_restore;
#[cfg(test)]
#[path = "suites/dialog_content_sizing.rs"]
mod dialog_content_sizing;
#[cfg(test)]
#[path = "suites/dialog_context_spacing.rs"]
mod dialog_context_spacing;
#[cfg(test)]
#[path = "suites/dialog_controls.rs"]
mod dialog_controls;
#[cfg(test)]
#[path = "suites/dialog_header_layout.rs"]
mod dialog_header_layout;
#[cfg(test)]
#[path = "suites/dialog_page_navigation.rs"]
mod dialog_page_navigation;
#[cfg(test)]
#[path = "suites/dialog_pinned_context.rs"]
mod dialog_pinned_context;
#[cfg(test)]
#[path = "suites/global_quit_aliases.rs"]
mod global_quit_aliases;
#[cfg(test)]
#[path = "suites/harness_contract.rs"]
mod harness_contract;
#[cfg(test)]
#[path = "suites/heading_style.rs"]
mod heading_style;
#[cfg(test)]
#[path = "suites/help_controls.rs"]
mod help_controls;
#[cfg(test)]
#[path = "suites/hit_geometry.rs"]
mod hit_geometry;
#[cfg(test)]
#[path = "suites/input_isolation.rs"]
mod input_isolation;
#[cfg(all(test, feature = "test-support", debug_assertions))]
#[path = "suites/input_receipts.rs"]
mod input_receipts;
#[cfg(test)]
#[path = "suites/list_controls.rs"]
mod list_controls;
#[path = "suites/list_hover.rs"]
mod list_hover;
#[cfg(test)]
#[path = "suites/list_navigation_policy.rs"]
mod list_navigation_policy;
#[cfg(test)]
#[path = "suites/list_typeahead.rs"]
mod list_typeahead;
#[cfg(test)]
#[path = "suites/live_actions.rs"]
mod live_actions;
#[path = "suites/menu_layout.rs"]
mod menu_layout;
#[cfg(test)]
#[path = "suites/scroll_geometry.rs"]
mod scroll_geometry;
#[cfg(test)]
mod support;
#[cfg(test)]
#[path = "suites/surface_exceptions.rs"]
mod surface_exceptions;
#[cfg(test)]
#[path = "suites/switch_controls.rs"]
mod switch_controls;
#[cfg(test)]
#[path = "suites/toolbar_labels.rs"]
mod toolbar_labels;
#[cfg(test)]
#[path = "suites/typography_coverage.rs"]
mod typography_coverage;
#[cfg(test)]
#[path = "suites/wheel_isolation.rs"]
mod wheel_isolation;

#[cfg(test)]
#[path = "suites/dialog_context_content.rs"]
mod dialog_context_content;

#[cfg(test)]
#[path = "suites/dialog_result_extent.rs"]
mod dialog_result_extent;

#[cfg(test)]
#[path = "suites/combo_option_layout.rs"]
mod combo_option_layout;

#[cfg(test)]
#[path = "suites/field_reason_layout.rs"]
mod field_reason_layout;
