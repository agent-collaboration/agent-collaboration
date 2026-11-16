import type { LedgerSection } from "../schema-audit-types.ts";

export const section: LedgerSection = {
  root: "crates/codex-router-quota-reset-test-harness",
  files: [
    {
      file: "crates/codex-router-quota-reset-test-harness/src/bin/quota-login-recording-standin.rs",
      disposition: { kind: "none" },
      note: 'synthetic login child for hermetic PTY proof: Serialize-only LoginTerminalSnapshot (:17) and LoginStandinReceipt (:29) written as JSON to a private fixture file (:78) and a private Unix gate socket (:108) that expects b"settle\\n" (:112). Harness-private receipt, not Codex app-server, ACP, MCP or Router vocabulary.',
    },
    {
      file: "crates/codex-router-quota-reset-test-harness/src/lib.rs",
      disposition: { kind: "none" },
      note: "feature-gated run_sessions_picker_test_harness forwarding to agent_collaboration (:10-17), loopback_provider module (:20), TestResult alias (:22); no serde, no message shapes.",
    },
    {
      file: "crates/codex-router-quota-reset-test-harness/src/loopback_provider.rs",
      disposition: { kind: "none" },
      note: "loopback HTTP stand-in for the ChatGPT backend quota API (OpenAI HTTP quota API, out of scope): parses the request line and ChatGPT-Account-ID header into SafeRequestRecord (:20, :327-343) and returns literal JSON bodies for POST consume {code, windows_reset} (:288), GET /usage rate_limit/code_review_rate_limit/credits/spend_control (:290, :292) and reset-credit inventory {credits[], available_count} (:294). Not Codex app-server account/rateLimits, not ACP/MCP.",
    },
    {
      file: "crates/codex-router-quota-reset-test-harness/src/main.rs",
      disposition: { kind: "none" },
      note: "binary entry: --sessions-picker dispatch (:4-8) or codex_router_cli::run_quota_reset_test_harness on a current-thread runtime (:9-19); no serde, no message shapes.",
    },
  ],
  sendSites: [],
};
