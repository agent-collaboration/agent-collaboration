import type { LedgerSection } from "../schema-audit-types.ts";

export const section: LedgerSection = {
  root: "crates/codex-router-core",
  files: [
    {
      file: "crates/codex-router-core/src/affinity.rs",
      disposition: { kind: "none" },
      note: "previous-response affinity: PreviousResponseId and serde AffinityKeyHash (45) newtypes for Router state, HMAC primitives; no message shape on the four protocols",
    },
    {
      file: "crates/codex-router-core/src/attempt_outcome.rs",
      disposition: { kind: "none" },
      note: "request attempt outcome enums (AttemptOutcome, PassThroughReason, TransportFailure), internal policy without serde",
    },
    {
      file: "crates/codex-router-core/src/audit.rs",
      disposition: {
        kind: "excluded",
        reason: "Router audit JSONL log, not one of the four audited protocols",
      },
      note: "AuditEvent (85) and its enums serialized as JSON lines by AuditFileSink (190); Router audit log, not one of the four protocols",
    },
    {
      file: "crates/codex-router-core/src/config.rs",
      disposition: { kind: "none" },
      note: "RouterConfig TOML file model (serde Deserialize 10-85), a Router config document, not a message",
    },
    {
      file: "crates/codex-router-core/src/credit_usage.rs",
      disposition: { kind: "none" },
      note: "credit accounting: CreditAvailability, CreditSpendControl and CreditProviderLimitReason with as_str/parse string tables (75,125-136,173-197) for ChatGPT usage-API reason values and Router state storage; the ChatGPT usage endpoint is not one of the four protocols",
    },
    {
      file: "crates/codex-router-core/src/error.rs",
      disposition: { kind: "none" },
      note: "error types",
    },
    {
      file: "crates/codex-router-core/src/ids.rs",
      disposition: { kind: "none" },
      note: "serde identifier newtypes for Router state (AccountId, RequestId, ReservationId, AffinityKey, TokenGeneration, RouteId); none is a session, thread, input or operation id on the four protocols",
    },
    {
      file: "crates/codex-router-core/src/lib.rs",
      disposition: { kind: "none" },
      note: "crate root, module declarations",
    },
    {
      file: "crates/codex-router-core/src/local_auth.rs",
      disposition: { kind: "none" },
      note: "local bearer token check for model routes; no message shape",
    },
    {
      file: "crates/codex-router-core/src/provider.rs",
      disposition: { kind: "none" },
      note: "Provider enum (openai, claude) with as_str/parse persisted names (21-37); Router state vocabulary, not one of the four protocols",
    },
    {
      file: "crates/codex-router-core/src/redaction.rs",
      disposition: { kind: "none" },
      note: "redaction wrappers (SecretString Serialize as redacted text); no message shape",
    },
    {
      file: "crates/codex-router-core/src/route_profile.rs",
      disposition: { kind: "none" },
      note: "route policy properties; no serde, no message shape",
    },
    {
      file: "crates/codex-router-core/src/router_compatibility.rs",
      disposition: {
        kind: "excluded",
        reason: "outside Project 2's protocols (Router GET /healthz)",
      },
      note: "RouterCompatibility (11), the GET /healthz body the Host reads (Host reader: codex-router-host router_compatibility.rs); an HTTP body between Router and Host, not one of the four protocols",
    },
    {
      file: "crates/codex-router-core/src/routes.rs",
      disposition: { kind: "none" },
      note: "RouteBand enum and route name as_str/parse (29-46) for Router state; not one of the four protocols",
    },
  ],
  sendSites: [],
};
