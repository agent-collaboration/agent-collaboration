//! grouping owns its picker modules and colocated checks.

#[cfg(test)]
pub(crate) mod picker_grouping_component_test;

#[cfg(test)]
pub(crate) mod picker_grouping_persistence_test;

#[cfg(test)]
pub(crate) mod picker_grouping_projection_test;

pub(crate) mod picker_grouping;

pub mod session_group_preferences;

pub(crate) mod session_grouping;
