#[cfg(test)]
#[path = "suites/action_controls.rs"]
mod action_controls;
#[cfg(test)]
#[path = "suites/combo_controls.rs"]
mod combo_controls;
#[cfg(test)]
#[path = "suites/control_editor.rs"]
mod control_editor;
#[cfg(test)]
#[path = "suites/dialog_controls.rs"]
mod dialog_controls;
#[cfg(test)]
#[path = "suites/harness_contract.rs"]
mod harness_contract;
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
#[cfg(test)]
#[path = "suites/list_navigation_policy.rs"]
mod list_navigation_policy;
#[cfg(test)]
#[path = "suites/live_actions.rs"]
mod live_actions;
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
#[path = "suites/typography_coverage.rs"]
mod typography_coverage;
#[cfg(test)]
#[path = "suites/wheel_isolation.rs"]
mod wheel_isolation;
