//! The pure state machines of provider sessions and their operations (D1
//! state machines, owner Decision 5): each is a reducer over state-machine-
//! core, with no I/O, no locks and no clock reads. The connection, the Host
//! and their cells hold the state and run the effects.

pub mod monotonic_instant;
mod provider_model_reasoning;
mod provider_setting_choice;
pub mod provider_setting_kind;
pub mod provider_settings_catalog;
pub mod queue_line_machine;
pub mod queued_entry_machine;
mod session_cell_machine;
pub mod session_generation;
pub mod session_lifecycle_answers;
pub mod session_lifecycle_machine;
pub mod session_owner;
pub mod session_refusal_answers;
pub mod session_setting_machine;
pub mod session_turn_machine;
pub mod tool_ownership;

pub use monotonic_instant::MonotonicInstant;
pub use provider_model_reasoning::{
    ModelReasoning, ModelReasoningEntry, ModelReasoningMetadata, ModelReasoningSelect,
    ModelReasoningUnknown,
};
pub use provider_setting_choice::ProviderSettingChoice;
pub use provider_setting_kind::ProviderSettingKind;
pub use provider_settings_catalog::{
    EffectiveProviderSettings, FastObserved, ProviderConfigOption, ProviderConfigValue,
    ProviderModeObservation, ProviderSettingsCatalog,
};
pub use session_generation::SessionGeneration;
