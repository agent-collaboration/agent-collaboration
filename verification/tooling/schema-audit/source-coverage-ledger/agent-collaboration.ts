import type { LedgerSection, SendSite } from "../schema-audit-types.ts";
// Left out by name only: 12 colocated unit-test files (*_test.rs, *_tests.rs). Part 1 of 2 by path order (acp_stdio_bridge.rs to claude_terminal_test_harness.rs); part 2 is agent-collaboration-2.ts.

const CLI_CALLER_REASON =
  "Caller of a collaboration-client method whose tools/call is posted by api_connection.rs:141; no node tap records frames from CLI callers (the N11 tap posts its own request).";
const AC = "crates/agent-collaboration/src/";
/** One CLI call into a CollaborationClient method that posts one MCP tools/call; tool defaults to the method name (verified in collaboration-client). */
const cliToolCall = (
  file: string,
  line: number,
  method: string,
  tool: string = method,
): SendSite => ({
  file,
  line,
  edge: "mcp",
  sends: `tools/call ${tool} via CollaborationClient::${method}`,
  owner: { crate: "agent-collaboration" },
  origin: "product",
  coverage: { kind: "uncovered", reason: CLI_CALLER_REASON },
});

export const section: LedgerSection = {
  root: "crates/agent-collaboration",
  files: [
    {
      file: "crates/agent-collaboration/src/acp_stdio_bridge.rs",
      disposition: { kind: "sends", owners: ["acp"] },
      note: "agent-collaboration acp: forward_json_lines L95 copies newline-delimited ACP frames byte for byte (writer.write_all L123, 64 MiB FRAME_LIMIT L7) in both directions: stdin to the endpoint's ACP carrier (L79, the outside-edge write) and carrier to stdout (L80, the ACP client that launched the bridge reads it, as native_stdio_bridge.rs:150 lists its stdout direction; send site at L80). Never parses ids, initializes or answers callbacks (L121).",
    },
    {
      file: "crates/agent-collaboration/src/address_book_commands.rs",
      disposition: { kind: "none" },
      note: "addresses list: clap AddressArguments L17 / AddressCommand L22; calls client.list_addresses L91 and prints the typed snapshot through result_envelope(json!(snapshot)) L102 (plain serialization of one type) or text L105-L125. No hand-built or hand-converted message shape.",
    },
    {
      file: "crates/agent-collaboration/src/approval_commands.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "approval list/decide: --allow/--allow-for-session/--deny flags to ApprovalDecision L128-L136; hand error envelopes {kind:error,error:OperationFailure} L219, L237; parse_actor L249 decodes typed Identity, or a bare SessionRef wrapped as Identity::Session when no kind field. Typed approval records serialized with to_value L179-L196; OperationFailureKind to exit codes L62.",
    },
    {
      file: "crates/agent-collaboration/src/automation_argument_feedback.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "parse_arguments: clap failure under --json becomes a hand-built invalidField error envelope with json! L18-L28 (kind, operationId, error{kind,stage,message,field,constraint,effects,nextAction}).",
    },
    {
      file: "crates/agent-collaboration/src/automation_collection_commands.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "Read-only automation collections: clap option structs L16-L76 to typed requests in prepare L268 (UUIDv7 ids by TryFrom); typed results serialized by read_value L260; hand error envelopes json! L236, L237 and the unsupportedCapability/unavailable body L239-L245.",
    },
    {
      file: "crates/agent-collaboration/src/automation_configuration_commands.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "automation configure/status: AutomationConfigureRequest L64; result_envelope of a hand {operationId,result} L153; hand error envelopes L163 (typed ConfigurationFailure) and L170 (outcomeUnknown/unavailable chosen from dispatched state, no stage field).",
    },
    {
      file: "crates/agent-collaboration/src/automation_history_commands.rs",
      disposition: { kind: "none" },
      note: "delivery/revision/operation clap commands L9-L120 that only map subcommands onto automation_collection_commands::CollectionCommand; no message shape.",
    },
    {
      file: "crates/agent-collaboration/src/bin/agent-sessions.rs",
      disposition: { kind: "none" },
      note: "agent-sessions binary entry: --version / --help, then agent_collaboration::run_arguments L21.",
    },
    {
      file: "crates/agent-collaboration/src/central_commands.rs",
      disposition: {
        kind: "excluded",
        reason:
          "Central owner admin HTTP protocol (collaboration_central_admin_protocol via CentralAdminClient L245), between the CLI and central, not one of the six outside edges or four protocols: builds OwnerMove L109, OwnerTakeover L145, OwnerRetirement L160, CopyPageQuery L98 and decodes TakeoverPreview from a file L140.",
      },
      note: "Also maps CentralCommandFailure to CLI error kinds and exit codes L178-L191.",
    },
    {
      file: "crates/agent-collaboration/src/central_migration.rs",
      disposition: {
        kind: "excluded",
        reason:
          "Central owner admin protocol migration sweep output, not one of the six outside edges or four protocols: Serialize shapes MigrationCounts L10, MigrationStop L33, MigrationCompletion L61, MigrationEvent L70 wrap CopyMigrationItem/OwnerOperationFailure from collaboration_central_admin_protocol.",
      },
      note: "Declared by #[path] at central_commands.rs:280.",
    },
    {
      file: "crates/agent-collaboration/src/central_output.rs",
      disposition: {
        kind: "excluded",
        reason:
          "Central owner admin protocol answers (CentralAdminAnswer, OwnerTransferAnswer, OwnerRetirementAnswer, OwnerOperationFailure) printed by print_answer L40 with exit codes; not one of the six outside edges or four protocols.",
      },
      note: "Declared by #[path] at central_commands.rs:274.",
    },
    {
      file: "crates/agent-collaboration/src/claude_terminal_test_harness.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "Built only with the quota-reset-test-harness feature (lib.rs:6-7), not cfg(test): a stand-in endpoint inventory reply hand-built with json! L39-L44 and decoded by serde_json::from_value into the inventory type, then qualify_capabilities L46.",
    },
  ],
  sendSites: [
    {
      file: "crates/agent-collaboration/src/acp_stdio_bridge.rs",
      line: 51,
      edge: "mcp",
      sends:
        "tools/call endpoints_list via AcpTransportConnection::connect (collaboration_access.rs:125 calls CollaborationClient::list_endpoints) to find the ACP carrier",
      owner: { crate: "agent-collaboration" },
      origin: "product",
      coverage: { kind: "uncovered", reason: CLI_CALLER_REASON },
    },
    {
      file: "crates/agent-collaboration/src/acp_stdio_bridge.rs",
      line: 123,
      edge: "acp-served-n8",
      sends:
        "ACP JSONL frames relayed unparsed from stdin to the endpoint's ACP carrier (forward_json_lines called at L79)",
      owner: { crate: "agent-collaboration" },
      origin: "product",
      coverage: {
        kind: "uncovered",
        reason:
          "tests/integration/acp_bridge_command.rs:26 checks the relay byte for byte, but no node tap records the relayed frames; the acp_carrier_tool_to_n8 tap goes through collaboration-client, not this bridge.",
      },
    },
    {
      file: "crates/agent-collaboration/src/acp_stdio_bridge.rs",
      line: 80,
      edge: "acp-served-n8",
      sends:
        "ACP JSONL frames from the endpoint's ACP carrier relayed unparsed to stdout (the ACP client side), through the same forward_json_lines writer L123",
      owner: { crate: "agent-collaboration" },
      origin: "product",
      coverage: {
        kind: "uncovered",
        reason:
          "tests/integration/acp_bridge_command.rs:26 checks the relay byte for byte, but no node tap records the relayed frames; the acp_carrier_tool_to_n8 tap goes through collaboration-client, not this bridge.",
      },
    },
    cliToolCall(`${AC}address_book_commands.rs`, 91, "list_addresses", "addresses_list"),
    cliToolCall(`${AC}approval_commands.rs`, 181, "decide_approval", "approval_decide"),
    cliToolCall(`${AC}approval_commands.rs`, 187, "list_approvals_with_options", "approval_list"),
    cliToolCall(`${AC}approval_commands.rs`, 193, "list_pending_approvals", "approval_list"),
  ],
};
