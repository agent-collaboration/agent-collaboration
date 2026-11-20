/**
 * The D0 code inventory (R51, Program Design Y4): every place our code defines or converts a
 * message shape by hand, its duplicate group, who uses it and who fixes it. Seeded from the
 * evidence sweep (tmp/p2-d0-schema-audit/evidence/), completed by the source audit, reviewed
 * against the code. Each deliverable removes the rows it fixes.
 */
import type { InventoryRow } from "./schema-audit-types.ts";
import { rows as acpClientRuntime } from "./definition-inventory/acp-client-runtime.ts";
import { rows as acpProviderAdapters } from "./definition-inventory/acp-provider-adapters.ts";
import { rows as agentAutomation } from "./definition-inventory/agent-automation.ts";
import { rows as agentCollaboration2 } from "./definition-inventory/agent-collaboration-2.ts";
import { rows as agentCollaboration } from "./definition-inventory/agent-collaboration.ts";
import { rows as agentSessionInventory } from "./definition-inventory/agent-session-inventory.ts";
import { rows as agentSessionTui } from "./definition-inventory/agent-session-tui.ts";
import { rows as boardFleetSync } from "./definition-inventory/board-fleet-sync.ts";
import { rows as boardSyncProtocol } from "./definition-inventory/board-sync-protocol.ts";
import { rows as claudeCodePeerMessaging } from "./definition-inventory/claude-code-peer-messaging.ts";
import { rows as codexAcpAdapter } from "./definition-inventory/codex-acp-adapter.ts";
import { rows as codexNativeIntegration } from "./definition-inventory/codex-native-integration.ts";
import { rows as codexRouterAuth } from "./definition-inventory/codex-router-auth.ts";
import { rows as codexRouterCli } from "./definition-inventory/codex-router-cli.ts";
import { rows as codexRouterCore } from "./definition-inventory/codex-router-core.ts";
import { rows as codexRouterDebugHosts } from "./definition-inventory/codex-router-debug-hosts.ts";
import { rows as codexRouterHost2 } from "./definition-inventory/codex-router-host-2.ts";
import { rows as codexRouterHost } from "./definition-inventory/codex-router-host.ts";
import { rows as codexRouterProxy } from "./definition-inventory/codex-router-proxy.ts";
import { rows as codexRouterTestSupport } from "./definition-inventory/codex-router-test-support.ts";
import { rows as collaborationApplicationTestSupport } from "./definition-inventory/collaboration-application-test-support.ts";
import { rows as collaborationCentralConformance } from "./definition-inventory/collaboration-central-conformance.ts";
import { rows as collaborationCentralCore } from "./definition-inventory/collaboration-central-core.ts";
import { rows as collaborationClient } from "./definition-inventory/collaboration-client.ts";
import { rows as collaborationContracts } from "./definition-inventory/collaboration-contracts.ts";
import { rows as collaborationMailProtocol } from "./definition-inventory/collaboration-mail-protocol.ts";
import { rows as collaborationMcp } from "./definition-inventory/collaboration-mcp.ts";
import { rows as collaborationProtocol2 } from "./definition-inventory/collaboration-protocol-2.ts";
import { rows as collaborationProtocol } from "./definition-inventory/collaboration-protocol.ts";
import { rows as collaborationRuntime1 } from "./definition-inventory/collaboration-runtime-1.ts";
import { rows as collaborationRuntime2 } from "./definition-inventory/collaboration-runtime-2.ts";
import { rows as collaborationService2 } from "./definition-inventory/collaboration-service-2.ts";
import { rows as collaborationService3 } from "./definition-inventory/collaboration-service-3.ts";
import { rows as collaborationService } from "./definition-inventory/collaboration-service.ts";
import { rows as fleetIdentityProtocol } from "./definition-inventory/fleet-identity-protocol.ts";
import { rows as interactionStorage } from "./definition-inventory/interaction-storage.ts";
import { rows as lifecycleObservation } from "./definition-inventory/lifecycle-observation.ts";
import { rows as messageBoardStorage } from "./definition-inventory/message-board-storage.ts";
import { rows as messageBoard2 } from "./definition-inventory/message-board-2.ts";
import { rows as messageBoard } from "./definition-inventory/message-board.ts";
import { rows as providersClaudeAcp } from "./definition-inventory/providers-claude-acp.ts";
import { rows as sessionEventModel } from "./definition-inventory/session-event-model.ts";
import { rows as tailnetVerificationHost } from "./definition-inventory/tailnet-verification-host.ts";

export const DEFINITION_INVENTORY: readonly InventoryRow[] = [
  ...acpClientRuntime,
  ...acpProviderAdapters,
  ...agentAutomation,
  ...agentCollaboration2,
  ...agentCollaboration,
  ...agentSessionInventory,
  ...agentSessionTui,
  ...boardFleetSync,
  ...boardSyncProtocol,
  ...claudeCodePeerMessaging,
  ...codexAcpAdapter,
  ...codexNativeIntegration,
  ...codexRouterAuth,
  ...codexRouterCli,
  ...codexRouterCore,
  ...codexRouterDebugHosts,
  ...codexRouterHost2,
  ...codexRouterHost,
  ...codexRouterProxy,
  ...codexRouterTestSupport,
  ...collaborationApplicationTestSupport,
  ...collaborationCentralConformance,
  ...collaborationCentralCore,
  ...collaborationClient,
  ...collaborationContracts,
  ...collaborationMailProtocol,
  ...collaborationMcp,
  ...collaborationProtocol,
  ...collaborationProtocol2,
  ...collaborationRuntime1,
  ...collaborationRuntime2,
  ...collaborationService2,
  ...collaborationService3,
  ...collaborationService,
  ...fleetIdentityProtocol,
  ...interactionStorage,
  ...lifecycleObservation,
  ...messageBoardStorage,
  ...messageBoard,
  ...messageBoard2,
  ...providersClaudeAcp,
  ...sessionEventModel,
  ...tailnetVerificationHost,
];
