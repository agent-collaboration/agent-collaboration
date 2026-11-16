/**
 * The Program Design's producer/consumer table ("Who produces and consumes each message",
 * docs/specs/2026-11-13-protocol-contracts/program-design.html#producers-consumers), as data.
 * Each family keeps the table's own wording: who produces it, who consumes it, and the nodes the
 * table names as intermediaries (N5 carries session events; N7 forwards decisions to N6 and N8;
 * N10 sends Codex requests on behalf of N8 and other users).
 */

export type Party =
  | "N1"
  | "N3"
  | "N4"
  | "N5"
  | "N6"
  | "N7"
  | "N8"
  | "N9"
  | "N10"
  | "N11"
  | "N12"
  | "CodexCli"
  | "ManagedCodex"
  | "AcpAgent"
  | "McpCaller"
  | "Inventory"
  | "CodexClientUser"
  /** The provider supervisor's ProviderConversationBackend (N11 and automation bypass N4 today; D3). */
  | "ProviderSupervisor";

export type MessageFamily =
  | "codexClientRequestFromCli"
  | "codexMessageToCli"
  | "codexRequestToCodex"
  | "codexMessageFromCodex"
  | "sessionCommand"
  | "promptContents"
  | "acpToAgent"
  | "acpSessionUpdate"
  | "sessionEvent"
  | "approvalRequest"
  | "approvalDecision"
  | "acpRequestToCodexAdapter"
  | "acpServedForCodexSessions"
  | "acpServedForRouterSessions"
  | "mcpToolCall"
  | "mcpToolSchema"
  | "claudeModelMetadata";

export interface ProducerConsumerRow {
  readonly message: string;
  readonly producers: readonly Party[];
  readonly consumers: readonly Party[];
  /** Nodes the table names between producer and consumer; they both take and pass the message. */
  readonly intermediaries: readonly Party[];
}

export const PRODUCER_CONSUMER_TABLE: Readonly<Record<MessageFamily, ProducerConsumerRow>> = {
  codexClientRequestFromCli: {
    message: "Codex client request (from CLI)",
    producers: ["CodexCli"],
    consumers: ["N1"],
    intermediaries: [],
  },
  codexMessageToCli: {
    message: "Codex response, notification, server request (to CLI)",
    producers: ["N3", "N1"],
    consumers: ["CodexCli"],
    // N3 hands its frames to N1, which sends them (N1 · CLI face node section).
    intermediaries: ["N1"],
  },
  codexRequestToCodex: {
    message: "Codex request (to Codex)",
    producers: ["N8", "N11", "CodexClientUser"],
    consumers: ["ManagedCodex"],
    // "each through N10" / "N10 → Managed Codex".
    intermediaries: ["N10"],
  },
  codexMessageFromCodex: {
    message: "Codex notification, server request (from Codex)",
    producers: ["ManagedCodex"],
    consumers: ["N8", "CodexClientUser"],
    intermediaries: ["N10"],
  },
  sessionCommand: {
    message: "Session command (prompt, steer, settings, cancel)",
    producers: ["N1", "N11", "N9"],
    consumers: ["N6"],
    // "N4 → N6".
    intermediaries: ["N4"],
  },
  promptContents: {
    message: "Prompt contents",
    producers: ["N4"],
    consumers: ["N6"],
    intermediaries: [],
  },
  acpToAgent: {
    message: "ACP prompt, set config/mode, steering extension",
    producers: ["N6"],
    consumers: ["AcpAgent"],
    intermediaries: [],
  },
  acpSessionUpdate: {
    message: "ACP session update, and the agent's own requests (permission)",
    producers: ["AcpAgent"],
    consumers: ["N6"],
    intermediaries: [],
  },
  sessionEvent: {
    message: "Session event",
    producers: ["N6", "N7"],
    consumers: ["N3", "N11", "N9", "Inventory", "N4"],
    intermediaries: ["N5"],
  },
  approvalRequest: {
    message: "Approval request",
    producers: ["N6", "N8"],
    consumers: ["N7"],
    intermediaries: [],
  },
  approvalDecision: {
    message: "Approval decision",
    producers: ["N1", "N11"],
    consumers: ["N6", "N8"],
    // "N7 → N6, N8".
    intermediaries: ["N7"],
  },
  acpRequestToCodexAdapter: {
    message: "ACP request to the Codex ACP adapter",
    producers: ["McpCaller", "N11"],
    consumers: ["N8"],
    intermediaries: [],
  },
  acpServedForCodexSessions: {
    message: "ACP served for Codex sessions",
    producers: ["N8"],
    consumers: ["McpCaller"],
    intermediaries: [],
  },
  acpServedForRouterSessions: {
    message: "ACP served for Router sessions (requests in, updates out)",
    // "MCP callers ⇄ N9": both sides produce and consume.
    producers: ["McpCaller", "N9"],
    consumers: ["N9", "McpCaller"],
    intermediaries: [],
  },
  mcpToolCall: {
    message: "MCP tool call and tools/list",
    producers: ["McpCaller"],
    consumers: ["N11"],
    intermediaries: [],
  },
  mcpToolSchema: {
    message: "MCP tool schema",
    producers: ["N11"],
    consumers: ["McpCaller"],
    intermediaries: [],
  },
  claudeModelMetadata: {
    message: "Claude model metadata",
    producers: ["N12"],
    consumers: ["N6"],
    intermediaries: [],
  },
};
