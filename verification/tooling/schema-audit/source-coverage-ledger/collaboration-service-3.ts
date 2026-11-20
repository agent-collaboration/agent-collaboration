import type { LedgerSection } from "../schema-audit-types.ts";

const P = "crates/collaboration-service/src/";
const SD = `${P}subscription_delivery/`;
export const section: LedgerSection = {
  root: "crates/collaboration-service",
  files: [
    {
      file: `${P}provider_session_inventory.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "MCP provider_sessions_list: ProviderInventoryCursor JSON in a base64 cursor string (L16, decode L205, encode L384); view_name table (L37); SessionState to ProviderSessionState (L45); protocol SessionRef to board SessionRef by to_value/from_value (L322); literal claude-local endpoint check (L174)",
    },
    {
      file: `${P}response_budget.rs`,
      disposition: { kind: "none" },
      note: "ResultByteBudget measures serde_json::to_vec length of any result (L37); defines no shape",
    },
    {
      file: `${P}run_projection.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "snapshot builds MCP RunSnapshot: agent_automation CapturedRunInputs and WorkerOutcome to protocol types by to_value/from_value (L21, L30); Claude peer session id string compare (L82); RunPhase to RunState with literal explanations (L116-126)",
    },
    {
      file: `${P}schedule_activation.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "ExecutionDestination to collaboration_contracts ScheduleDestination (L31-74); OwnedThread SessionRef flattened to BindingAddress service/endpoint/thread strings (L57-62)",
    },
    {
      file: `${P}schedule_preparation.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "MCP schedule_prepare: DestinationPreparation to ScheduleDestination (L42); JSON bytes of (scheduleId, destination) as canonical request (L135); stored failure Value decoded to ScheduleFailure (L174-178); ScheduleFailure built by hand (L342, L371, L398, L477)",
    },
    {
      file: `${P}schedule_projection.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "snapshot converts stored ScheduleDefinition (L11) and imported continuity (L15) to MCP ScheduleSnapshot types by to_value/from_value",
    },
    {
      file: `${SD}intent_read_cost_counter.rs`,
      disposition: {
        kind: "excluded",
        reason: `cfg(test) module declared at ${SD}intent_progress_tests.rs:11-12, which is declared #[cfg(test)] at ${SD}subscription_service.rs:4-6`,
      },
      note: "tracing layer counting sqlx statements and record decodes for intent progress tests",
    },
  ],
  sendSites: [],
};
