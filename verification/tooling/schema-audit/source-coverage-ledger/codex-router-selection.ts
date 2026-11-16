import type { LedgerSection } from "../schema-audit-types.ts";

export const section: LedgerSection = {
  root: "crates/codex-router-selection",
  files: [
    {
      file: "crates/codex-router-selection/src/affinity.rs",
      disposition: { kind: "none" },
      note: "In-memory AffinityTable (L10-L33) pinning AffinityKey to AccountId for model-traffic account selection; no serde, no message shape of the four protocols.",
    },
    {
      file: "crates/codex-router-selection/src/burn_down.rs",
      disposition: { kind: "none" },
      note: "Quota burn-down policy constants L4-L46, #[path] module declarations L48-L67 and re-exports L69-L81; #[cfg(test)] tests module L83-L85. No message shape of the four protocols.",
    },
    {
      file: "crates/codex-router-selection/src/burn_down/account_assessment.rs",
      disposition: { kind: "none" },
      note: "assess_account L25-L327 computes one account's quota availability, routing weight and RoutingReason from in-process QuotaWindowFact inputs; floor exclusion and display metrics L329-L475. OpenAI/Claude quota policy, no serde, no message shape of the four protocols.",
    },
    {
      file: "crates/codex-router-selection/src/burn_down/assessment_input.rs",
      disposition: { kind: "none" },
      note: "In-process assessment inputs BurnDownRouteBandAssessmentInput L11-L52, CreditBackedEligibility L21-L27, BurnDownAccountInput L56-L209, QuotaWindowRejectionFact L213-L251; no serde, no message shape of the four protocols.",
    },
    {
      file: "crates/codex-router-selection/src/burn_down/assessment_result.rs",
      disposition: { kind: "none" },
      note: "In-process assessment outputs BurnDownRouteBandAssessmentResult L19-L64, BurnDownAccountAssessment L68-L289 and status enums AccountAvailability/SelectedPool/QuotaEvidenceFreshness/RoutingExclusion/LimitingWindow L293-L378; no serde, no message shape of the four protocols.",
    },
    {
      file: "crates/codex-router-selection/src/burn_down/burn_down_tests/quota_account_fixtures.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/codex-router-selection/src/burn_down/burn_down_tests.rs:10-11 (#[path] mod quota_account_fixtures), inside the tests module declared #[cfg(test)] at burn_down.rs:83-85",
      },
      note: "Quota account and window test builders; no product code.",
    },
    {
      file: "crates/codex-router-selection/src/burn_down/burn_down_tests/scenario_selection_fixtures.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/codex-router-selection/src/burn_down/burn_down_tests.rs:12-13 (#[path] mod scenario_selection_fixtures), inside the tests module declared #[cfg(test)] at burn_down.rs:83-85",
      },
      note: "Account-selection scenario runner for tests; no product code.",
    },
    {
      file: "crates/codex-router-selection/src/burn_down/candidate_priority.rs",
      disposition: { kind: "none" },
      note: "candidate_priority_cmp L21-L40 and weekly survival/drain tie-break comparators L42-L293 over BurnDownAccountAssessment; pure ordering, no message shape of the four protocols.",
    },
    {
      file: "crates/codex-router-selection/src/burn_down/credit_assessment.rs",
      disposition: { kind: "none" },
      note: "Credit-backed Responses reserve admission rules (canonical_responses_compact_window_assessments L12-L36, credit_backed_candidate_is_eligible L52-L87); pure quota policy, no message shape of the four protocols.",
    },
    {
      file: "crates/codex-router-selection/src/burn_down/route_assessment.rs",
      disposition: { kind: "none" },
      note: "assess_route_band L23-L194 composes account assessments into the selected pool and weighted candidates; admission/far-idle/credit hold rules L196-L392. Pure quota policy, no message shape of the four protocols.",
    },
    {
      file: "crates/codex-router-selection/src/burn_down/routing_policy.rs",
      disposition: { kind: "none" },
      note: "BurnDownRouteBandPolicy L38-L64, weekly floor switch points L8-L34 and bounded numeric helpers L66-L142; no message shape of the four protocols. In-file #[cfg(test)] module L144.",
    },
    {
      file: "crates/codex-router-selection/src/burn_down/routing_reasons.rs",
      disposition: { kind: "none" },
      note: "QuotaEvidenceReason L9-L34 and RoutingReason L38-L87 with stable machine codes as_str L92-L119 and human phrases L123-L154 for quota routing explanations, assigned by routing_reason_for_account L218-L312. Quota-routing explanation for quota UI/CLI; collaboration-protocol and collaboration-mcp do not depend on this crate (consumers per Cargo.toml: codex-router-cli, codex-router-proxy, codex-router-quota-service, codex-router-state, agent-router-quota-tui), so not one of the four protocols.",
    },
    {
      file: "crates/codex-router-selection/src/burn_down/window_assessment.rs",
      disposition: { kind: "none" },
      note: "Per-window pressure, survival, drain and salvage arithmetic (assess_window L60-L102, helpers L104-L395) over in-process QuotaWindowFact; no message shape of the four protocols.",
    },
    {
      file: "crates/codex-router-selection/src/burn_down/window_facts.rs",
      disposition: { kind: "none" },
      note: "In-process QuotaWindowStatus L7-L16 and QuotaWindowFact builder L20-L209; no serde, no message shape of the four protocols.",
    },
    {
      file: "crates/codex-router-selection/src/eligibility.rs",
      disposition: { kind: "none" },
      note: "SelectionCandidate::eligibility L37-L64 classifies headroom and freshness into Eligibility L69-L87 with static audit reasons; no message shape of the four protocols.",
    },
    {
      file: "crates/codex-router-selection/src/lib.rs",
      disposition: { kind: "none" },
      note: "Module declarations L3-L11 and package_name L25-L29; #[cfg(test)] modules L13-L23 and L31. No message shape of the four protocols.",
    },
    {
      file: "crates/codex-router-selection/src/precommit.rs",
      disposition: { kind: "none" },
      note: "PrecommitFailure L5-L14 to PrecommitRotationDecision L18-L23 classification L27-L48 for model-traffic account rotation; no message shape of the four protocols.",
    },
    {
      file: "crates/codex-router-selection/src/reservation.rs",
      disposition: { kind: "none" },
      note: "In-memory ReservationBook L59-L188 and ReservationHandle L10-L48 keyed by ReservationId (counter-formatted reservation_<n> L79); account headroom reservations, not a Router-vocabulary identity of the four protocols. In-file #[cfg(test)] module L190.",
    },
    {
      file: "crates/codex-router-selection/src/run_rate.rs",
      disposition: { kind: "none" },
      note: "QuotaRunRateEstimator L179-L260 over QuotaRunRateObservation L23-L61 yielding QuotaRunRateEstimate L65-L175; pure arithmetic, no message shape of the four protocols.",
    },
    {
      file: "crates/codex-router-selection/src/selection_outcome.rs",
      disposition: { kind: "none" },
      note: "SelectionOutcome L56-L113, SelectionAccountState L245-L349 and UnavailableReason L385-L421 with R12 precedence classify_unavailable_reason L425-L487; in-process account-selection result, no serde, no message shape of the four protocols (not the collaboration-protocol UnavailableReason, which is a separate type). #[cfg(test)] module L509-L511.",
    },
    {
      file: "crates/codex-router-selection/src/turn_state.rs",
      disposition: { kind: "none" },
      note: "TurnStateEnvelopeCodec L71-L150 signs TurnStatePayload (Serialize+Deserialize L152-L156: account_id, upstream_token) as base64url JSON plus HMAC-SHA256 (encode L83-L100, decode_str L108-L125). No product caller outside this crate (only lib.rs #[cfg(test)] module L176-L197 uses it), so no header or frame on any of the six edges carries it; a private signed token format, not one of the four protocols.",
    },
    {
      file: "crates/codex-router-selection/src/weighted_deficit.rs",
      disposition: { kind: "none" },
      note: "Smooth weighted-deficit WeightedDeficitSelector L62-L135 and SelectionDecision L11-L58; in-memory selection, no message shape of the four protocols.",
    },
  ],
  sendSites: [],
};
