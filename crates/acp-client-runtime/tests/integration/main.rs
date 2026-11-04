//! Original test roots retain their case namespaces.

#[path = "../support/provider_behavior.rs"]
mod test_behavior;

mod approval_offer_cutover;
mod create_settings_order;
mod cursor_model_settings;
mod cursor_plan_approval;
mod cursor_question_roundtrip;
mod cursor_todo_events;
mod history_replay_order;
mod interaction_cancel_wire_order;
mod provider_auth_status;
mod provider_behavior_integration;
mod provider_elicitation;
mod provider_event_overflow;
mod provider_idle_updates;
mod provider_input_events;
mod provider_lifecycle;
mod provider_multiblock_prompt;
mod provider_settings_contract;
mod provider_transport_idle;
mod provider_turn_boundaries;
mod unknown_stop_output;
