/**
 * The finite set of catalog rows D0's node taps must cover, quoted from the Program Design
 * (docs/specs/2026-11-13-protocol-contracts/program-design.html: the conversion catalog, the
 * dispositions table, the node sections N1–N12 and the producer/consumer table). Each tap case
 * names one of these rows exactly as its `catalog_row`; the report fails on a required row with no
 * recording and on a recording whose row is not listed here.
 *
 * DECLARED_GAPS lists design rows D0 cannot tap, each with its reason; the report prints them so
 * the denominator stays visible instead of silently shrinking.
 *
 * N2 is the opaque byte relay and has no taps by design.
 */
import type { MessageFamily, Party } from "./producer-consumer-table.ts";

export interface RequiredCatalogRow {
  readonly node: Party;
  readonly row: string;
}

export interface DeclaredGap extends RequiredCatalogRow {
  readonly reason: string;
  /** The deliverable that owns closing the gap (assignment Correction 9). */
  /** Who fixes it: D1, D2, the later CLI project, or an owner outside Project 2. */
  readonly owner: "D1" | "D2" | "cli-project" | "external:cli-mcp-lane";
}

function rows(node: Party, labels: readonly string[]): RequiredCatalogRow[] {
  return labels.map((row) => ({ node, row }));
}

export const REQUIRED_CATALOG_ROWS: readonly RequiredCatalogRow[] = [
  ...rows("N1", [
    "Dispositions: Ignored ClientNotificationWithoutId",
    "Dispositions: Minimal CompactUnsupported",
    "Dispositions: Refused AccountLimits",
    "Dispositions: Refused CodexMemory",
    "Dispositions: Refused CodexReview",
    "Dispositions: Refused ConfigTrustReadOnly",
    "Dispositions: Refused ConfigWrite (config/batchWrite)",
    "Dispositions: Refused ConfigWrite (config/write)",
    "Dispositions: Refused ContextCloneOrInject",
    "Dispositions: Refused HistoryRevert",
    "Dispositions: Refused HookDiscovery",
    "Dispositions: Refused HostShell",
    "Dispositions: Refused NativeQueue",
    "Dispositions: Refused PermissionProfileDiscovery",
    "Dispositions: Refused ProviderAuth",
    "Dispositions: Refused Realtime",
    "Dispositions: Refused SessionMetadataReadOnly",
    "Dispositions: Refused ThreadGoals",
    "Dispositions: change: no request is answered method not found",
    "N1 node: approval replies -> N7",
    "N1 node: session commands -> N4",
  ]),
  ...rows("N3", [
    "N3 catalog: CapabilitiesChanged, ResyncRequired",
    "N3 catalog: InputAccepted",
    "N3 catalog: InteractionRequested",
    "N3 catalog: InteractionResolved",
    "N3 catalog: Item ModeChange, ConfigChange, SessionInfo, CommandCatalog",
    "N3 catalog: Item Plan",
    "N3 catalog: Item Usage",
    "N3 catalog: ItemCompleted",
    "N3 catalog: ItemStarted, other kinds",
    "N3 catalog: ItemUpdated",
    "N3 catalog: SettingsChanged",
    "N3 catalog: StateChanged",
    "N3 catalog: TurnEnded",
    "N3 catalog: TurnStarted",
    "N3 node section: receipt state machine",
  ]),
  ...rows("N4", [
    "N4: Session command (cancel) -> cancel to N6",
    "N4: Session command (prompt) -> prompt contents to N6; waits for TurnStarted from N5",
    "N4: Session command (settings) -> set config option to N6",
    "N4: Session command (steer) -> steer contents to N6",
    "N4: Session command (steer) with no matching active turn -> nothing to N6",
    "N4: content capability check (provider_content_commands.rs:318), accepting",
    "N4: content capability check (provider_content_commands.rs:318), refusing",
  ]),
  ...rows("N5", [
    "N5 node: N7 interaction events -> hub -> subscribers",
    "N5 node: available commands arrive as a JSON string inside SessionItem.text (D2)",
    "N5 node: broadcast + history; attach replays the snapshot",
    "N5 node: config options arrive as a JSON string inside SessionItem.text (D2)",
    "N5 node: events reach N3, N11, inventory and N4",
    "N5 node: session info arrives as a JSON string inside SessionItem.text (D2)",
    "N5 node: usage arrives as a JSON string inside SessionItem.text (D2)",
  ]),
  ...rows("N6", [
    "N6 catalog: Typed extension messages: Cursor model discovery, any failure means no model data",
    "N6 catalog: Typed extension messages: Cursor model discovery, models reply with unknown fields",
    "N6 catalog: Typed extension messages: Steering, failed",
    "N6 catalog: Typed extension messages: Steering, injected while running",
    "N6 catalog: Typed extension messages: Steering, outcome outside the closed enum is an invalid-reply error",
    "N6 catalog: Typed extension messages: Steering, promptRequired",
    "N6 catalog: Typed extension messages: Steering, startedNewTurn while idle",
    "N6 catalog: A non-text block closes the open item",
    "N6 catalog: Available commands update: same events, plus the skill-view update (unchanged)",
    "N6 catalog: Available commands update; change: CommandCatalog typed (today a Notice whose text is JSON)",
    "N6 catalog: Config option update; change: ConfigChange carries typed config options, not a JSON string",
    "N6 catalog: Current mode update: 0 or 1 ItemCompleted for open text, then Started + Completed: 2-3 events",
    "N6 catalog: Current mode update: Started + Completed: 2 events; ModeChange { mode id }",
    "N6 catalog: Other transitions: Compaction promotion emits one Updated",
    "N6 catalog: Other transitions: Turn start and end clear the turn-local cursors but keep the tool items",
    "N6 catalog: Other transitions: finish() completes the text, then the tools, then the plan",
    "N6 catalog: Plan: 1 Updated (subsequent); one stable id per turn",
    "N6 catalog: Plan: closes open text, then 1 Started (first)",
    "N6 catalog: Session info update; change: SessionInfo carries typed fields",
    "N6 catalog: Tool call update: 1 Started (unknown id); unknown id starts as other / pending",
    "N6 catalog: Tool call update: 1 Updated (known id)",
    "N6 catalog: Tool call update: contextCompaction kind is kept",
    "N6 catalog: Tool call: 1 Updated (known id)",
    "N6 catalog: Tool call: closes open text, then 1 Started (new id)",
    "N6 catalog: Unknown update kind (decode level): Started + Completed: 2 events; Unknown { source kind }",
    "N6 catalog: Unknown update kind: 0 or 1 ItemCompleted for open text, then Started + Completed",
    "N6 catalog: Usage update; change: Usage { used, size } typed",
    "N6 catalog: User / agent message chunk, thought chunk",
    "N6 catalog: User / agent message chunk, thought chunk (id = message id or a new UUIDv7)",
    "N6 catalog: continuing text: 1 ItemUpdated with the full text so far",
    "N6 catalog: new text item: 1 ItemStarted, preceded by 1 ItemCompleted if the previous text item differs",
    "N7 section and S3: N6 permission request -> N7 broker; N7 selected option -> N6 -> ACP outcome",
  ]),
  ...rows("N7", [
    "N7: approval request in from N6; interaction events to N5; decision in; selected option out to N6",
    "N7: decision in (cancel); cancelled out to N6; interaction events to N5",
    "N7: the broker settles each request once; the first decision wins",
  ]),
  ...rows("N8", [
    "Content conversions: N8 prompt_content_translation.rs",
    "Content conversions: N8 64 MiB limit",
    "N8 section: ACP prompt from the carrier -> Codex calls to N10",
    "N8 section: Codex notifications from N10 -> ACP updates (terminal response) to the carrier",
    "N8 section: Codex notifications from N10 -> ACP updates to the carrier",
    "N8 section: legacy approval path; table: Approval request N8 -> N7",
    // The wire case on the public ACP connection (acp_served_n8_node_tap.rs): one prompt end to end.
    "N8 section: ACP prompt from the carrier -> Codex calls to N10 -> ACP updates and the terminal response to the carrier",
  ]),
  ...rows("N9", [
    "N9 node: ACP client request -> session command -> response",
    "N9 node: session event rendered as ACP session/update",
    "N9 node: session/prompt -> Prompt command; TurnEnded -> stop reason response",
    "N9 node: shares N3's metadata filter (ProviderMetadataNotTranscript)",
  ]),
  ...rows("N10", [
    "N10 section: request<R> -> R::Response or CodexCallError; R17 malformed replies",
  ]),
  ...rows("N11", [
    "N11 section: Codex shapes spliced into tool schemas at runtime (schema_binding.rs:3)",
    "N11 section: N11 --> N10; table: Codex request (to Codex) from N10",
    "N11 section: N11 --> N4; table: session command N1, N11 -> N4",
    "N11 section: N11 --ACP--> N8",
  ]),
  ...rows("N12", [
    "Claude model reasoning v1: available models (model id, config id, choices, default)",
    "Claude model reasoning v1: unknown with a reason (UnsupportedSource ...)",
    "Retained difference: Zod pins version 1, Rust reports other versions as Unrecognized",
    "Retained difference: Zod strips unknown keys, Rust rejects them",
  ]),
];

export const DECLARED_GAPS: readonly DeclaredGap[] = [
  {
    node: "N6",
    row: "N6 catalog: Typed extension messages: steering 10 second timeout and Cursor discovery 2 second timeout",
    owner: "external:cli-mcp-lane",
    reason:
      "both read Tokio time directly; with no injected clock a tap would be a wall-clock test",
  },
  {
    node: "N6",
    row: "N6 catalog: Any other variant (the SDK enum may grow): closes open text only",
    owner: "D1",
    reason:
      "agent-client-protocol is built with default features, so SessionUpdate has no variant that reaches the fallback arm; the D25 schema check owns it",
  },
  {
    node: "N7",
    row: "N7: pending approvals, 300s timeout",
    owner: "external:cli-mcp-lane",
    reason:
      "the port sleeps on Tokio time directly; with no injected clock a tap would be a wall-clock test",
  },
  {
    node: "N3",
    row: "N3 node section: receipt Bound -> Dropped when the send fails",
    owner: "D2",
    reason:
      "the drop happens in N1's connection loop, which D0 cannot observe without a product change: NativeUserReceipts lives in the loop's local forwarding (router_session_app_server.rs:238), the failed websocket.send returns before delivered (:345-346) and drops the whole connection state, and a send-only failure over UnixStream::pair also ends the read half; N3's node_tap_receipt_bound_item_is_not_retired_before_delivery proves the receipt stays outstanding without delivery",
  },
];

/** A wired path the table rules out, recorded as a current structural defect with its owner. */
export interface DeclaredStructuralDefect {
  readonly node: Party;
  readonly family: MessageFamily;
  readonly counterpart: Party;
  /** The declared orientation: only this direction and reply flag are the defect. */
  readonly direction: "produced" | "consumed";
  readonly reply: boolean;
  /** Who fixes it: D1, D2, the later CLI project, or an owner outside Project 2. */
  readonly owner: "D1" | "D2" | "cli-project" | "external:cli-mcp-lane";
  readonly defect: string;
  /** Code anchors from the one-time send-site review (tmp/d0-verification/implementer/s5-send-site-review.md). */
  readonly anchors: readonly string[];
}

export const DECLARED_STRUCTURAL_DEFECTS: readonly DeclaredStructuralDefect[] = [
  {
    node: "N11",
    family: "sessionCommand",
    counterpart: "ProviderSupervisor",
    direction: "produced",
    reply: false,
    owner: "external:cli-mcp-lane",
    defect:
      "N11's conversation operations and the automation paths call the provider supervisor's ProviderConversationBackend directly, bypassing N4 (Program Design #N4, assignment Correction 10)",
    anchors: [
      "crates/collaboration-service/src/collaboration_application/conversation_operations.rs:55-62",
      "crates/codex-router-host/src/external_provider_supervisor/provider_conversation_backend.rs:23,347",
      "crates/codex-router-host/src/provider_acp_scheduled_runs/provider_acp_run_dispatch.rs:164,282",
      "crates/codex-router-host/src/provider_acp_message_fifo.rs:317",
      "crates/codex-router-host/src/provider_acp_session_loading.rs:126",
    ],
  },
];
