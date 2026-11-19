//! Real C5 owner coordination; only the external native client is scripted.

#[path = "follow_admission_proofs.rs"]
mod follow_admission_proofs;
#[path = "incoming_poll_session.rs"]
mod incoming_poll_session;
#[path = "membership_expiry_admission_proofs.rs"]
mod membership_expiry_admission_proofs;
#[path = "native_cancel_backpressure_proofs.rs"]
mod native_cancel_backpressure_proofs;
#[path = "native_cancel_takeover_fixture.rs"]
mod native_cancel_takeover_fixture;
#[path = "native_follow_cancel_proofs.rs"]
mod native_follow_cancel_proofs;
#[path = "native_follow_expiry_proofs.rs"]
mod native_follow_expiry_proofs;
#[path = "native_follow_grouping_proofs.rs"]
mod native_follow_grouping_proofs;
#[path = "native_follow_injection_proofs.rs"]
mod native_follow_injection_proofs;
#[path = "native_follow_mail_proofs.rs"]
pub(super) mod native_follow_mail_proofs;
#[path = "native_follow_outcome_fault_proofs.rs"]
mod native_follow_outcome_fault_proofs;
#[path = "native_frontier_proofs.rs"]
mod native_frontier_proofs;
#[path = "native_generation_isolation_proofs.rs"]
mod native_generation_isolation_proofs;
#[path = "native_generation_prevention_proofs.rs"]
mod native_generation_prevention_proofs;
#[path = "native_handoff_recovery_proofs.rs"]
mod native_handoff_recovery_proofs;
#[path = "native_held_mail_fault_proofs.rs"]
mod native_held_mail_fault_proofs;
#[path = "native_held_mail_proofs.rs"]
mod native_held_mail_proofs;
#[path = "native_intent_publisher_proofs.rs"]
mod native_intent_publisher_proofs;
#[path = "../../../tests/support/native_mail_owner.rs"]
pub(super) mod native_mail_owner;
#[path = "native_owed_transfer_proofs.rs"]
mod native_owed_transfer_proofs;
#[path = "native_poll_gate_proofs.rs"]
mod native_poll_gate_proofs;
#[path = "native_poll_incoming_proofs.rs"]
mod native_poll_incoming_proofs;
#[path = "native_poll_mail_proofs.rs"]
mod native_poll_mail_proofs;
#[path = "native_poll_recovery_proofs.rs"]
mod native_poll_recovery_proofs;
#[path = "native_readiness_interleaving_proofs.rs"]
mod native_readiness_interleaving_proofs;
#[path = "native_successor_frontier_proofs.rs"]
mod native_successor_frontier_proofs;
#[path = "native_window_lineage_proofs.rs"]
mod native_window_lineage_proofs;
#[path = "window_lineage_fixture.rs"]
mod window_lineage_fixture;
