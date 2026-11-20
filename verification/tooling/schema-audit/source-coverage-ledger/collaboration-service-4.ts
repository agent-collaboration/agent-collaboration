import type { LedgerSection } from "../schema-audit-types.ts";

export const section: LedgerSection = {
  root: "crates/collaboration-service",
  files: [
    {
      file: "crates/collaboration-service/src/subscription_delivery/tests/follow_admission_proofs.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/subscription_delivery/tests/local_mail_proofs.rs:10 (#[path] mod), local_mail_proofs declared at crates/collaboration-service/src/subscription_delivery/tests.rs:9 whose ancestor `mod tests` is #[cfg(test)] at crates/collaboration-service/src/subscription_delivery/mod.rs:25",
      },
      note: "unit-test code over Router-internal board/mail/delivery fixtures; no Codex, ACP or MCP frame is built, sent or decoded",
    },
    {
      file: "crates/collaboration-service/src/subscription_delivery/tests/incoming_poll_session.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/subscription_delivery/tests/local_mail_proofs.rs:12 (#[path] mod), local_mail_proofs declared at crates/collaboration-service/src/subscription_delivery/tests.rs:9 whose ancestor `mod tests` is #[cfg(test)] at crates/collaboration-service/src/subscription_delivery/mod.rs:25",
      },
      note: "test stand-in for the central fleet session server: decodes ClientFrame<BoardConsumerProtocol> and answers ServerFrame (l45, l71, l89); central board/fleet protocol, not one of the six outside edges",
    },
    {
      file: "crates/collaboration-service/src/subscription_delivery/tests/lifecycle_proofs.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/subscription_delivery/tests.rs:8 inside `mod tests`, #[cfg(test)] at crates/collaboration-service/src/subscription_delivery/mod.rs:25",
      },
      note: "unit-test code over Router-internal board/mail/delivery fixtures; no Codex, ACP or MCP frame is built, sent or decoded",
    },
    {
      file: "crates/collaboration-service/src/subscription_delivery/tests/local_mail_proofs.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/subscription_delivery/tests.rs:9 inside `mod tests`, #[cfg(test)] at crates/collaboration-service/src/subscription_delivery/mod.rs:25",
      },
      note: "unit-test code over Router-internal board/mail/delivery fixtures; no Codex, ACP or MCP frame is built, sent or decoded",
    },
    {
      file: "crates/collaboration-service/src/subscription_delivery/tests/membership_expiry_admission_proofs.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/subscription_delivery/tests/local_mail_proofs.rs:14 (#[path] mod), local_mail_proofs declared at crates/collaboration-service/src/subscription_delivery/tests.rs:9 whose ancestor `mod tests` is #[cfg(test)] at crates/collaboration-service/src/subscription_delivery/mod.rs:25",
      },
      note: "unit-test code over Router-internal board/mail/delivery fixtures; no Codex, ACP or MCP frame is built, sent or decoded",
    },
    {
      file: "crates/collaboration-service/src/subscription_delivery/tests/native_activation_frontier.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/subscription_delivery/tests/native_frontier_proofs.rs:7-8 (#[path] mod activation_history), native_frontier_proofs declared at crates/collaboration-service/src/subscription_delivery/tests/local_mail_proofs.rs:32, local_mail_proofs at crates/collaboration-service/src/subscription_delivery/tests.rs:9 whose ancestor `mod tests` is #[cfg(test)] at crates/collaboration-service/src/subscription_delivery/mod.rs:25",
      },
      note: "unit-test code over Router-internal board/mail/delivery fixtures; no Codex, ACP or MCP frame is built, sent or decoded",
    },
    {
      file: "crates/collaboration-service/src/subscription_delivery/tests/native_cancel_backpressure_proofs.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/subscription_delivery/tests/local_mail_proofs.rs:16 (#[path] mod), local_mail_proofs declared at crates/collaboration-service/src/subscription_delivery/tests.rs:9 whose ancestor `mod tests` is #[cfg(test)] at crates/collaboration-service/src/subscription_delivery/mod.rs:25",
      },
      note: "unit-test code over Router-internal board/mail/delivery fixtures; no Codex, ACP or MCP frame is built, sent or decoded",
    },
    {
      file: "crates/collaboration-service/src/subscription_delivery/tests/native_cancel_takeover_fixture.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/subscription_delivery/tests/local_mail_proofs.rs:18 (#[path] mod), local_mail_proofs declared at crates/collaboration-service/src/subscription_delivery/tests.rs:9 whose ancestor `mod tests` is #[cfg(test)] at crates/collaboration-service/src/subscription_delivery/mod.rs:25",
      },
      note: "test fixture over fleet_sync_protocol lease/snapshot types (l3-l11); central fleet protocol, not an outside edge",
    },
    {
      file: "crates/collaboration-service/src/subscription_delivery/tests/native_follow_cancel_proofs.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/subscription_delivery/tests/local_mail_proofs.rs:20 (#[path] mod), local_mail_proofs declared at crates/collaboration-service/src/subscription_delivery/tests.rs:9 whose ancestor `mod tests` is #[cfg(test)] at crates/collaboration-service/src/subscription_delivery/mod.rs:25",
      },
      note: "unit-test code over Router-internal board/mail/delivery fixtures; no Codex, ACP or MCP frame is built, sent or decoded",
    },
    {
      file: "crates/collaboration-service/src/subscription_delivery/tests/native_follow_expiry_proofs.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/subscription_delivery/tests/local_mail_proofs.rs:22 (#[path] mod), local_mail_proofs declared at crates/collaboration-service/src/subscription_delivery/tests.rs:9 whose ancestor `mod tests` is #[cfg(test)] at crates/collaboration-service/src/subscription_delivery/mod.rs:25",
      },
      note: "unit-test code over Router-internal board/mail/delivery fixtures; no Codex, ACP or MCP frame is built, sent or decoded",
    },
    {
      file: "crates/collaboration-service/src/subscription_delivery/tests/native_follow_grouping_proofs.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/subscription_delivery/tests/local_mail_proofs.rs:24 (#[path] mod), local_mail_proofs declared at crates/collaboration-service/src/subscription_delivery/tests.rs:9 whose ancestor `mod tests` is #[cfg(test)] at crates/collaboration-service/src/subscription_delivery/mod.rs:25",
      },
      note: "unit-test code over Router-internal board/mail/delivery fixtures; no Codex, ACP or MCP frame is built, sent or decoded",
    },
    {
      file: "crates/collaboration-service/src/subscription_delivery/tests/native_follow_injection_proofs.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/subscription_delivery/tests/local_mail_proofs.rs:26 (#[path] mod), local_mail_proofs declared at crates/collaboration-service/src/subscription_delivery/tests.rs:9 whose ancestor `mod tests` is #[cfg(test)] at crates/collaboration-service/src/subscription_delivery/mod.rs:25",
      },
      note: "unit-test code over Router-internal board/mail/delivery fixtures; no Codex, ACP or MCP frame is built, sent or decoded",
    },
    {
      file: "crates/collaboration-service/src/subscription_delivery/tests/native_follow_mail_proofs.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/subscription_delivery/tests/local_mail_proofs.rs:28 (#[path] mod), local_mail_proofs declared at crates/collaboration-service/src/subscription_delivery/tests.rs:9 whose ancestor `mod tests` is #[cfg(test)] at crates/collaboration-service/src/subscription_delivery/mod.rs:25",
      },
      note: "unit-test code over Router-internal board/mail/delivery fixtures; no Codex, ACP or MCP frame is built, sent or decoded",
    },
    {
      file: "crates/collaboration-service/src/subscription_delivery/tests/native_follow_outcome_fault_proofs.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/subscription_delivery/tests/local_mail_proofs.rs:30 (#[path] mod), local_mail_proofs declared at crates/collaboration-service/src/subscription_delivery/tests.rs:9 whose ancestor `mod tests` is #[cfg(test)] at crates/collaboration-service/src/subscription_delivery/mod.rs:25",
      },
      note: "unit-test code over Router-internal board/mail/delivery fixtures; no Codex, ACP or MCP frame is built, sent or decoded",
    },
    {
      file: "crates/collaboration-service/src/subscription_delivery/tests/native_frontier_proofs.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/subscription_delivery/tests/local_mail_proofs.rs:32 (#[path] mod), local_mail_proofs declared at crates/collaboration-service/src/subscription_delivery/tests.rs:9 whose ancestor `mod tests` is #[cfg(test)] at crates/collaboration-service/src/subscription_delivery/mod.rs:25",
      },
      note: "unit-test code over Router-internal board/mail/delivery fixtures; no Codex, ACP or MCP frame is built, sent or decoded",
    },
    {
      file: "crates/collaboration-service/src/subscription_delivery/tests/native_generation_isolation_proofs.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/subscription_delivery/tests/local_mail_proofs.rs:34 (#[path] mod), local_mail_proofs declared at crates/collaboration-service/src/subscription_delivery/tests.rs:9 whose ancestor `mod tests` is #[cfg(test)] at crates/collaboration-service/src/subscription_delivery/mod.rs:25",
      },
      note: "unit-test code over Router-internal board/mail/delivery fixtures; no Codex, ACP or MCP frame is built, sent or decoded",
    },
    {
      file: "crates/collaboration-service/src/subscription_delivery/tests/native_generation_prevention_proofs.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/subscription_delivery/tests/local_mail_proofs.rs:36 (#[path] mod), local_mail_proofs declared at crates/collaboration-service/src/subscription_delivery/tests.rs:9 whose ancestor `mod tests` is #[cfg(test)] at crates/collaboration-service/src/subscription_delivery/mod.rs:25",
      },
      note: "unit-test code over Router-internal board/mail/delivery fixtures; no Codex, ACP or MCP frame is built, sent or decoded",
    },
    {
      file: "crates/collaboration-service/src/subscription_delivery/tests/native_handoff_recovery_proofs.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/subscription_delivery/tests/local_mail_proofs.rs:38 (#[path] mod), local_mail_proofs declared at crates/collaboration-service/src/subscription_delivery/tests.rs:9 whose ancestor `mod tests` is #[cfg(test)] at crates/collaboration-service/src/subscription_delivery/mod.rs:25",
      },
      note: "unit-test code over Router-internal board/mail/delivery fixtures; no Codex, ACP or MCP frame is built, sent or decoded",
    },
    {
      file: "crates/collaboration-service/src/subscription_delivery/tests/native_held_mail_fault_proofs.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/subscription_delivery/tests/local_mail_proofs.rs:40 (#[path] mod), local_mail_proofs declared at crates/collaboration-service/src/subscription_delivery/tests.rs:9 whose ancestor `mod tests` is #[cfg(test)] at crates/collaboration-service/src/subscription_delivery/mod.rs:25",
      },
      note: "unit-test code over Router-internal board/mail/delivery fixtures; no Codex, ACP or MCP frame is built, sent or decoded",
    },
    {
      file: "crates/collaboration-service/src/subscription_delivery/tests/native_held_mail_proofs.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/subscription_delivery/tests/local_mail_proofs.rs:42 (#[path] mod), local_mail_proofs declared at crates/collaboration-service/src/subscription_delivery/tests.rs:9 whose ancestor `mod tests` is #[cfg(test)] at crates/collaboration-service/src/subscription_delivery/mod.rs:25",
      },
      note: "unit-test code over Router-internal board/mail/delivery fixtures; no Codex, ACP or MCP frame is built, sent or decoded",
    },
    {
      file: "crates/collaboration-service/src/subscription_delivery/tests/native_intent_publisher_proofs.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/subscription_delivery/tests/local_mail_proofs.rs:44 (#[path] mod), local_mail_proofs declared at crates/collaboration-service/src/subscription_delivery/tests.rs:9 whose ancestor `mod tests` is #[cfg(test)] at crates/collaboration-service/src/subscription_delivery/mod.rs:25",
      },
      note: "unit-test code over Router-internal board/mail/delivery fixtures; no Codex, ACP or MCP frame is built, sent or decoded",
    },
    {
      file: "crates/collaboration-service/src/subscription_delivery/tests/native_mail_attempt_proofs.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/subscription_delivery/tests.rs:10 inside `mod tests`, #[cfg(test)] at crates/collaboration-service/src/subscription_delivery/mod.rs:25",
      },
      note: "unit-test code over Router-internal board/mail/delivery fixtures; no Codex, ACP or MCP frame is built, sent or decoded",
    },
    {
      file: "crates/collaboration-service/src/subscription_delivery/tests/native_owed_transfer_proofs.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/subscription_delivery/tests/local_mail_proofs.rs:48 (#[path] mod), local_mail_proofs declared at crates/collaboration-service/src/subscription_delivery/tests.rs:9 whose ancestor `mod tests` is #[cfg(test)] at crates/collaboration-service/src/subscription_delivery/mod.rs:25",
      },
      note: "unit-test code over Router-internal board/mail/delivery fixtures; no Codex, ACP or MCP frame is built, sent or decoded",
    },
    {
      file: "crates/collaboration-service/src/subscription_delivery/tests/native_poll_gate_proofs.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/subscription_delivery/tests/local_mail_proofs.rs:50 (#[path] mod), local_mail_proofs declared at crates/collaboration-service/src/subscription_delivery/tests.rs:9 whose ancestor `mod tests` is #[cfg(test)] at crates/collaboration-service/src/subscription_delivery/mod.rs:25",
      },
      note: "unit-test code over Router-internal board/mail/delivery fixtures; no Codex, ACP or MCP frame is built, sent or decoded",
    },
    {
      file: "crates/collaboration-service/src/subscription_delivery/tests/native_poll_incoming_proofs.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/subscription_delivery/tests/local_mail_proofs.rs:52 (#[path] mod), local_mail_proofs declared at crates/collaboration-service/src/subscription_delivery/tests.rs:9 whose ancestor `mod tests` is #[cfg(test)] at crates/collaboration-service/src/subscription_delivery/mod.rs:25",
      },
      note: "unit-test code over Router-internal board/mail/delivery fixtures; no Codex, ACP or MCP frame is built, sent or decoded",
    },
    {
      file: "crates/collaboration-service/src/subscription_delivery/tests/native_poll_mail_proofs.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/subscription_delivery/tests/local_mail_proofs.rs:54 (#[path] mod), local_mail_proofs declared at crates/collaboration-service/src/subscription_delivery/tests.rs:9 whose ancestor `mod tests` is #[cfg(test)] at crates/collaboration-service/src/subscription_delivery/mod.rs:25",
      },
      note: "unit-test code over Router-internal board/mail/delivery fixtures; no Codex, ACP or MCP frame is built, sent or decoded",
    },
    {
      file: "crates/collaboration-service/src/subscription_delivery/tests/native_poll_recovery_proofs.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/subscription_delivery/tests/local_mail_proofs.rs:56 (#[path] mod), local_mail_proofs declared at crates/collaboration-service/src/subscription_delivery/tests.rs:9 whose ancestor `mod tests` is #[cfg(test)] at crates/collaboration-service/src/subscription_delivery/mod.rs:25",
      },
      note: "unit-test code over Router-internal board/mail/delivery fixtures; no Codex, ACP or MCP frame is built, sent or decoded",
    },
    {
      file: "crates/collaboration-service/src/subscription_delivery/tests/native_readiness_interleaving_proofs.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/subscription_delivery/tests/local_mail_proofs.rs:58 (#[path] mod), local_mail_proofs declared at crates/collaboration-service/src/subscription_delivery/tests.rs:9 whose ancestor `mod tests` is #[cfg(test)] at crates/collaboration-service/src/subscription_delivery/mod.rs:25",
      },
      note: "unit-test code over Router-internal board/mail/delivery fixtures; no Codex, ACP or MCP frame is built, sent or decoded",
    },
    {
      file: "crates/collaboration-service/src/subscription_delivery/tests/native_successor_frontier_proofs.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/subscription_delivery/tests/local_mail_proofs.rs:60 (#[path] mod), local_mail_proofs declared at crates/collaboration-service/src/subscription_delivery/tests.rs:9 whose ancestor `mod tests` is #[cfg(test)] at crates/collaboration-service/src/subscription_delivery/mod.rs:25",
      },
      note: "unit-test code over Router-internal board/mail/delivery fixtures; no Codex, ACP or MCP frame is built, sent or decoded",
    },
    {
      file: "crates/collaboration-service/src/subscription_delivery/tests/native_window_lineage_proofs.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/subscription_delivery/tests/local_mail_proofs.rs:62 (#[path] mod), local_mail_proofs declared at crates/collaboration-service/src/subscription_delivery/tests.rs:9 whose ancestor `mod tests` is #[cfg(test)] at crates/collaboration-service/src/subscription_delivery/mod.rs:25",
      },
      note: "unit-test code over Router-internal board/mail/delivery fixtures; no Codex, ACP or MCP frame is built, sent or decoded",
    },
    {
      file: "crates/collaboration-service/src/subscription_delivery/tests/owner_activity_proofs.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/subscription_delivery/tests.rs:12 inside `mod tests`, #[cfg(test)] at crates/collaboration-service/src/subscription_delivery/mod.rs:25",
      },
      note: "unit-test code over Router-internal board/mail/delivery fixtures; no Codex, ACP or MCP frame is built, sent or decoded",
    },
    {
      file: "crates/collaboration-service/src/subscription_delivery/tests/owner_fixture.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/subscription_delivery/tests.rs:13 inside `mod tests`, #[cfg(test)] at crates/collaboration-service/src/subscription_delivery/mod.rs:25",
      },
      note: "unit-test code over Router-internal board/mail/delivery fixtures; no Codex, ACP or MCP frame is built, sent or decoded",
    },
    {
      file: "crates/collaboration-service/src/subscription_delivery/tests/owner_native_lifecycle.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/subscription_delivery/tests/owner_fixture.rs:21 (#[path] mod), owner_fixture declared at crates/collaboration-service/src/subscription_delivery/tests.rs:13 whose ancestor `mod tests` is #[cfg(test)] at crates/collaboration-service/src/subscription_delivery/mod.rs:25",
      },
      note: "unit-test code over Router-internal board/mail/delivery fixtures; no Codex, ACP or MCP frame is built, sent or decoded",
    },
    {
      file: "crates/collaboration-service/src/subscription_delivery/tests/owner_setup.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/subscription_delivery/tests/owner_fixture.rs:94 (#[path] mod), owner_fixture declared at crates/collaboration-service/src/subscription_delivery/tests.rs:13 whose ancestor `mod tests` is #[cfg(test)] at crates/collaboration-service/src/subscription_delivery/mod.rs:25",
      },
      note: "test fixture builds central board requests (workspace/board/project create) as json! then decodes them (l70-l100); central board protocol, not an outside edge",
    },
    {
      file: "crates/collaboration-service/src/subscription_delivery/tests/push_store_proofs.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/subscription_delivery/tests.rs:14 inside `mod tests`, #[cfg(test)] at crates/collaboration-service/src/subscription_delivery/mod.rs:25",
      },
      note: "unit-test code over Router-internal board/mail/delivery fixtures; no Codex, ACP or MCP frame is built, sent or decoded",
    },
    {
      file: "crates/collaboration-service/src/subscription_delivery/tests/queued_proofs.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/subscription_delivery/tests.rs:15 inside `mod tests`, #[cfg(test)] at crates/collaboration-service/src/subscription_delivery/mod.rs:25",
      },
      note: "unit-test code over Router-internal board/mail/delivery fixtures; no Codex, ACP or MCP frame is built, sent or decoded",
    },
    {
      file: "crates/collaboration-service/src/subscription_delivery/tests/reconcile_retry_proofs.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/subscription_delivery/subscription_service.rs:719-721 (#[cfg(test)] #[path] mod reconcile_retry_proofs)",
      },
      note: "unit-test code over Router-internal board/mail/delivery fixtures; no Codex, ACP or MCP frame is built, sent or decoded",
    },
    {
      file: "crates/collaboration-service/src/subscription_delivery/tests/restore_corruption_proofs.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/subscription_delivery/tests.rs:16 inside `mod tests`, #[cfg(test)] at crates/collaboration-service/src/subscription_delivery/mod.rs:25",
      },
      note: "unit-test code over Router-internal board/mail/delivery fixtures; no Codex, ACP or MCP frame is built, sent or decoded",
    },
    {
      file: "crates/collaboration-service/src/subscription_delivery/tests/scripted_delivery.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/subscription_delivery/tests/owner_fixture.rs:3 (#[path] mod), owner_fixture declared at crates/collaboration-service/src/subscription_delivery/tests.rs:13 whose ancestor `mod tests` is #[cfg(test)] at crates/collaboration-service/src/subscription_delivery/mod.rs:25",
      },
      note: "test SessionMessageDelivery stand-in (l50) recording layer_zero::DeliveryRequest in memory; writes no Codex/ACP/MCP frame",
    },
    {
      file: "crates/collaboration-service/src/subscription_delivery/tests/stall_diagnosis.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/subscription_delivery/tests.rs:17 inside `mod tests`, #[cfg(test)] at crates/collaboration-service/src/subscription_delivery/mod.rs:25",
      },
      note: "unit-test code over Router-internal board/mail/delivery fixtures; no Codex, ACP or MCP frame is built, sent or decoded",
    },
    {
      file: "crates/collaboration-service/src/subscription_delivery/tests/startup_fixture.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/subscription_delivery/tests.rs:18 inside `mod tests`, #[cfg(test)] at crates/collaboration-service/src/subscription_delivery/mod.rs:25",
      },
      note: "unit-test code over Router-internal board/mail/delivery fixtures; no Codex, ACP or MCP frame is built, sent or decoded",
    },
    {
      file: "crates/collaboration-service/src/subscription_delivery/tests/startup_recovery_regressions.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/subscription_delivery/subscription_service.rs:17-19 (#[cfg(test)] #[path] mod startup_recovery_regressions)",
      },
      note: "unit-test code over Router-internal board/mail/delivery fixtures; no Codex, ACP or MCP frame is built, sent or decoded",
    },
    {
      file: "crates/collaboration-service/src/subscription_delivery/tests/startup_retry_proofs.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/subscription_delivery/tests.rs:19 inside `mod tests`, #[cfg(test)] at crates/collaboration-service/src/subscription_delivery/mod.rs:25",
      },
      note: "unit-test code over Router-internal board/mail/delivery fixtures; no Codex, ACP or MCP frame is built, sent or decoded",
    },
    {
      file: "crates/collaboration-service/src/subscription_delivery/tests/subscription_observation_proofs.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/subscription_delivery/tests/control_view_proofs.rs:10-11 (#[path] mod native_observations), control_view_proofs declared at crates/collaboration-service/src/subscription_delivery/tests.rs:3 whose ancestor `mod tests` is #[cfg(test)] at crates/collaboration-service/src/subscription_delivery/mod.rs:25",
      },
      note: "unit-test code over Router-internal board/mail/delivery fixtures; no Codex, ACP or MCP frame is built, sent or decoded",
    },
    {
      file: "crates/collaboration-service/src/subscription_delivery/tests/timing_proofs.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/subscription_delivery/tests.rs:20 inside `mod tests`, #[cfg(test)] at crates/collaboration-service/src/subscription_delivery/mod.rs:25",
      },
      note: "unit-test code over Router-internal board/mail/delivery fixtures; no Codex, ACP or MCP frame is built, sent or decoded",
    },
    {
      file: "crates/collaboration-service/src/subscription_delivery/tests/transient_scan_proofs.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/subscription_delivery/tests.rs:21 inside `mod tests`, #[cfg(test)] at crates/collaboration-service/src/subscription_delivery/mod.rs:25",
      },
      note: "unit-test code over Router-internal board/mail/delivery fixtures; no Codex, ACP or MCP frame is built, sent or decoded",
    },
    {
      file: "crates/collaboration-service/src/subscription_delivery/tests/wait_proofs.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/subscription_delivery/tests.rs:22 inside `mod tests`, #[cfg(test)] at crates/collaboration-service/src/subscription_delivery/mod.rs:25",
      },
      note: "unit-test code over Router-internal board/mail/delivery fixtures; no Codex, ACP or MCP frame is built, sent or decoded",
    },
    {
      file: "crates/collaboration-service/src/subscription_delivery/tests/window_lineage_fixture.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/collaboration-service/src/subscription_delivery/tests/local_mail_proofs.rs:64 (#[path] mod), local_mail_proofs declared at crates/collaboration-service/src/subscription_delivery/tests.rs:9 whose ancestor `mod tests` is #[cfg(test)] at crates/collaboration-service/src/subscription_delivery/mod.rs:25",
      },
      note: "unit-test code over Router-internal board/mail/delivery fixtures; no Codex, ACP or MCP frame is built, sent or decoded",
    },
    {
      file: "crates/collaboration-service/src/wakeup_projection.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "projects agent_automation WakeRecord into the MCP wake_send/wake_show/wake_pause/resume/cancel output WakeSnapshot / WakeMutationResult (collaboration-mcp catalog_tools.rs:118-124): FireReceipt copy l17, WakeState enum mapping l26, latest_event_cursor JSON-in-string l34, TimingRule->TimingRequest serde round trip l48, retained DeliveryRecord<Value,..>->DeliveryRecord<SessionRef,..> serde round trip l74",
    },
    {
      file: "crates/collaboration-service/src/wakeup_subscription.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "backs MCP wake_wait_until_first_fire (collaboration-mcp wait_tools.rs:54) via wake_operations.rs:462: parses the JSON-tuple cursor string l79 and rebuilds it l113, l185; maps stored transition kind strings to protocol WakeChange l149 and decodes body.fire into FirstFire l151, copied into FireReceipt l163",
    },
  ],
  sendSites: [],
};
