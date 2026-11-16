import type { LedgerSection, SendSite } from "../schema-audit-types.ts";

const R = "crates/collaboration-application-test-support/src/";

const noKeptCapture =
  "test apparatus: no kept complete capture records this fixture's connection (the N10 and N11 taps build and record their own sockets: validated_native_request_node_tap_test.rs, mcp_server_node_tap_test.rs)";

/**
 * One board_fixture tool call. ServedCollaborationApi::call (collaboration-mcp test_support.rs:135)
 * posts an rmcp tools/call over the fixture's control.sock with UnixSocketHttpClient
 * (test_support.rs:167): a real MCP request over a Unix HTTP socket, not an in-process dispatch.
 */
function fixtureToolCall(line: number, tool: string): SendSite {
  return {
    file: `${R}board_fixture.rs`,
    line,
    edge: "mcp",
    sends: `tools/call ${tool} via ServedCollaborationApi::call (Unix HTTP, collaboration-mcp test_support.rs:167)`,
    owner: { crate: "collaboration-application-test-support" },
    origin: "apparatus",
    coverage: { kind: "uncovered", reason: noKeptCapture },
  };
}

export const section: LedgerSection = {
  root: "crates/collaboration-application-test-support",
  files: [
    {
      file: "crates/collaboration-application-test-support/src/accepted_snapshot.rs",
      disposition: {
        kind: "none",
      },
      note: "central fleet-sync snapshot baseline (NoCalls CallRouter l14 returns serde_json::Value from durable_steps); central replication, not one of the four protocols",
    },
    {
      file: "crates/collaboration-application-test-support/src/board_context_backend.rs",
      disposition: {
        kind: "converts",
        owners: ["codex"],
      },
      note: "test stand-in Codex native endpoint: hand-built schema bundle {definitions:{v2:...}} l45-51, endpoint inventory l57; parses each request from its accepted WebSocket l93 and writes the initialize/thread/read reply {id,result} back over it l104-110 (apparatus send site)",
    },
    {
      file: "crates/collaboration-application-test-support/src/board_fixture.rs",
      disposition: {
        kind: "defines",
        owners: ["mcp"],
      },
      note: "hand-built tool arguments sent by tool name through ServedCollaborationApi::call, which posts an rmcp tools/call over the fixture's control.sock Unix HTTP socket (collaboration-mcp test_support.rs:135-176), not in-process dispatch (l116-121, l226-241, l446-488; inventory row l478; seven apparatus send sites); request header {requestId,protocolVersion,actingFor,actor} l446-451",
    },
    {
      file: "crates/collaboration-application-test-support/src/board_poll_services.rs",
      disposition: {
        kind: "none",
      },
      note: "FixturePollServices l6 test poll services; no serde",
    },
    {
      file: "crates/collaboration-application-test-support/src/lib.rs",
      disposition: {
        kind: "none",
      },
      note: "module declarations (l3-9) only",
    },
    {
      file: "crates/collaboration-application-test-support/src/mail_cancel_expansions.rs",
      disposition: {
        kind: "none",
      },
      note: "builds board SessionRef recipients field by field for cancel-expansion fixtures (l32); constructs, does not convert between two types",
    },
    {
      file: "crates/collaboration-application-test-support/src/native_authority.rs",
      disposition: {
        kind: "none",
      },
      note: "native authority test double; no serde, no protocol shapes",
    },
    {
      file: "crates/collaboration-application-test-support/src/native_mail_owner.rs",
      disposition: {
        kind: "none",
      },
      note: "native mail owner test double; no serde, no protocol shapes",
    },
    {
      file: "crates/collaboration-application-test-support/src/native_registry.rs",
      disposition: {
        kind: "none",
      },
      note: "PendingRegistration l41 native registry test double; no serde",
    },
    {
      file: "crates/collaboration-application-test-support/src/registered_project.rs",
      disposition: {
        kind: "none",
      },
      note: "registered project fixture helpers; no serde",
    },
  ],
  sendSites: [
    {
      file: `${R}board_context_backend.rs`,
      line: 104,
      edge: "managed-codex",
      sends:
        "stand-in Codex app-server reply {id,result} over the accepted native WebSocket: {} to initialize, {thread:{id,cwd}} to thread/read with includeTurns false; the Router's N10 native connection is the counterparty",
      owner: { crate: "collaboration-application-test-support" },
      origin: "apparatus",
      coverage: { kind: "uncovered", reason: noKeptCapture },
    },
    fixtureToolCall(121, "repository_attach"),
    fixtureToolCall(232, "workspace_create"),
    fixtureToolCall(241, "board_create"),
    fixtureToolCall(475, "project_create"),
    fixtureToolCall(478, "role_grant"),
    fixtureToolCall(479, "workstream_create"),
    fixtureToolCall(488, "task_create"),
  ],
};
