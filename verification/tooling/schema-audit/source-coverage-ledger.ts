/**
 * The D0 source-coverage ledger (R51, Program Design Y4): a disposition for every file under each
 * crate's and provider package's src directory at the base commit, and every outside-edge send
 * site with the recording that covers it. It is reviewed data, made once and shrunk by each
 * deliverable; it is not a lasting source scanner.
 *
 * File list: `git ls-tree -r --name-only` at LEDGER_BASE.commit, filtered by LEDGER_BASE.roots,
 * committed as data in ledger-base-files.ts (the base commit is not in the public repository after
 * the squash, so the test reads the committed list, never git),
 * leaving out by name only colocated unit tests (LEDGER_BASE.leftOutByName). cfg(test)-only
 * modules are listed as excluded with where they are declared.
 */
import type { LedgerSection } from "./schema-audit-types.ts";
import { section as acpClientRuntime } from "./source-coverage-ledger/acp-client-runtime.ts";
import { section as acpProviderAdapters } from "./source-coverage-ledger/acp-provider-adapters.ts";
import { section as agentAutomation } from "./source-coverage-ledger/agent-automation.ts";
import { section as agentCollaboration2 } from "./source-coverage-ledger/agent-collaboration-2.ts";
import { section as agentCollaborationTui1 } from "./source-coverage-ledger/agent-collaboration-tui-1.ts";
import { section as agentCollaborationTui2 } from "./source-coverage-ledger/agent-collaboration-tui-2.ts";
import { section as agentCollaboration } from "./source-coverage-ledger/agent-collaboration.ts";
import { section as agentRouterQuotaTui } from "./source-coverage-ledger/agent-router-quota-tui.ts";
import { section as agentSessionInventory } from "./source-coverage-ledger/agent-session-inventory.ts";
import { section as agentSessionTui1 } from "./source-coverage-ledger/agent-session-tui-1.ts";
import { section as agentSessionTui2 } from "./source-coverage-ledger/agent-session-tui-2.ts";
import { section as automationStorage } from "./source-coverage-ledger/automation-storage.ts";
import { section as boardCentralTestSupport } from "./source-coverage-ledger/board-central-test-support.ts";
import { section as boardFleetSync } from "./source-coverage-ledger/board-fleet-sync.ts";
import { section as boardSyncProtocol } from "./source-coverage-ledger/board-sync-protocol.ts";
import { section as centralLinkClient } from "./source-coverage-ledger/central-link-client.ts";
import { section as centralLinkProtocol } from "./source-coverage-ledger/central-link-protocol.ts";
import { section as centralStorage } from "./source-coverage-ledger/central-storage.ts";
import { section as claudeCodePeerMessaging } from "./source-coverage-ledger/claude-code-peer-messaging.ts";
import { section as codexAcpAdapter } from "./source-coverage-ledger/codex-acp-adapter.ts";
import { section as codexNativeIntegration } from "./source-coverage-ledger/codex-native-integration.ts";
import { section as codexRouterAuth } from "./source-coverage-ledger/codex-router-auth.ts";
import { section as codexRouterCli } from "./source-coverage-ledger/codex-router-cli.ts";
import { section as codexRouterCore } from "./source-coverage-ledger/codex-router-core.ts";
import { section as codexRouterDebugHosts } from "./source-coverage-ledger/codex-router-debug-hosts.ts";
import { section as codexRouterHost1 } from "./source-coverage-ledger/codex-router-host-1.ts";
import { section as codexRouterHost2 } from "./source-coverage-ledger/codex-router-host-2.ts";
import { section as codexRouterProxy2 } from "./source-coverage-ledger/codex-router-proxy-2.ts";
import { section as codexRouterProxy } from "./source-coverage-ledger/codex-router-proxy.ts";
import { section as codexRouterQuotaResetTestHarness } from "./source-coverage-ledger/codex-router-quota-reset-test-harness.ts";
import { section as codexRouterQuotaService } from "./source-coverage-ledger/codex-router-quota-service.ts";
import { section as codexRouterQuota } from "./source-coverage-ledger/codex-router-quota.ts";
import { section as codexRouterSecretStore } from "./source-coverage-ledger/codex-router-secret-store.ts";
import { section as codexRouterSelection } from "./source-coverage-ledger/codex-router-selection.ts";
import { section as codexRouterState } from "./source-coverage-ledger/codex-router-state.ts";
import { section as codexRouterTestSupport } from "./source-coverage-ledger/codex-router-test-support.ts";
import { section as collaborationApplicationTestSupport } from "./source-coverage-ledger/collaboration-application-test-support.ts";
import { section as collaborationCentralAdminProtocol } from "./source-coverage-ledger/collaboration-central-admin-protocol.ts";
import { section as collaborationCentralCloudflare } from "./source-coverage-ledger/collaboration-central-cloudflare.ts";
import { section as collaborationCentralConformance } from "./source-coverage-ledger/collaboration-central-conformance.ts";
import { section as collaborationCentralCore1 } from "./source-coverage-ledger/collaboration-central-core-1.ts";
import { section as collaborationCentralCore2 } from "./source-coverage-ledger/collaboration-central-core-2.ts";
import { section as collaborationCentralCore3 } from "./source-coverage-ledger/collaboration-central-core-3.ts";
import { section as collaborationCentralCore4 } from "./source-coverage-ledger/collaboration-central-core-4.ts";
import { section as collaborationCentralCore5 } from "./source-coverage-ledger/collaboration-central-core-5.ts";
import { section as collaborationCentralTokio } from "./source-coverage-ledger/collaboration-central-tokio.ts";
import { section as collaborationClient } from "./source-coverage-ledger/collaboration-client.ts";
import { section as collaborationContracts } from "./source-coverage-ledger/collaboration-contracts.ts";
import { section as collaborationMailProtocol } from "./source-coverage-ledger/collaboration-mail-protocol.ts";
import { section as collaborationMcp } from "./source-coverage-ledger/collaboration-mcp.ts";
import { section as collaborationProtocol } from "./source-coverage-ledger/collaboration-protocol.ts";
import { section as collaborationRuntime1 } from "./source-coverage-ledger/collaboration-runtime-1.ts";
import { section as collaborationRuntime2 } from "./source-coverage-ledger/collaboration-runtime-2.ts";
import { section as collaborationService1 } from "./source-coverage-ledger/collaboration-service-1.ts";
import { section as collaborationService2 } from "./source-coverage-ledger/collaboration-service-2.ts";
import { section as collaborationService3 } from "./source-coverage-ledger/collaboration-service-3.ts";
import { section as collaborationService4 } from "./source-coverage-ledger/collaboration-service-4.ts";
import { section as fleetAdmission } from "./source-coverage-ledger/fleet-admission.ts";
import { section as fleetIdentityProtocol } from "./source-coverage-ledger/fleet-identity-protocol.ts";
import { section as interactionStorage } from "./source-coverage-ledger/interaction-storage.ts";
import { section as lifecycleObservation } from "./source-coverage-ledger/lifecycle-observation.ts";
import { section as messageBoardStorage1 } from "./source-coverage-ledger/message-board-storage-1.ts";
import { section as messageBoardStorage2 } from "./source-coverage-ledger/message-board-storage-2.ts";
import { section as messageBoard } from "./source-coverage-ledger/message-board.ts";
import { section as nodeTestFixture } from "./source-coverage-ledger/node-test-fixture.ts";
import { section as ownedProcessTestSupport } from "./source-coverage-ledger/owned-process-test-support.ts";
import { section as providersClaudeAcp } from "./source-coverage-ledger/providers-claude-acp.ts";
import { section as routerClock } from "./source-coverage-ledger/router-clock.ts";
import { section as sessionEventModel } from "./source-coverage-ledger/session-event-model.ts";
import { section as sqliteStorageMaintenance } from "./source-coverage-ledger/sqlite-storage-maintenance.ts";
import { section as sqlxDurableObjectCore } from "./source-coverage-ledger/sqlx-durable-object-core.ts";
import { section as sqlxDurableObject } from "./source-coverage-ledger/sqlx-durable-object.ts";
import { section as tailnetVerificationHost } from "./source-coverage-ledger/tailnet-verification-host.ts";
import { section as uiPreferenceStore } from "./source-coverage-ledger/ui-preference-store.ts";
import { section as workspaceArchitectureCheck } from "./source-coverage-ledger/workspace-architecture-check.ts";
import { section as workspaceIds } from "./source-coverage-ledger/workspace-ids.ts";
import { section as workspaceModel } from "./source-coverage-ledger/workspace-model.ts";
import { section as workspaceRules } from "./source-coverage-ledger/workspace-rules.ts";

export const LEDGER_BASE = {
  /** The commit the committed list was generated from (ledger-base-files.ts). */
  commit: "afc76c82cfad3375e2861b1720084aae0530a6f4",
  roots: ["crates/*/src/**", "providers/*/src/**"],
  leftOutByName: ["*_test.rs", "*_tests.rs"],
} as const;

export const LEDGER_SECTIONS: readonly LedgerSection[] = [
  acpClientRuntime,
  acpProviderAdapters,
  agentAutomation,
  agentCollaboration2,
  agentCollaborationTui1,
  agentCollaborationTui2,
  agentCollaboration,
  agentRouterQuotaTui,
  agentSessionInventory,
  agentSessionTui1,
  agentSessionTui2,
  automationStorage,
  boardCentralTestSupport,
  boardFleetSync,
  boardSyncProtocol,
  centralLinkClient,
  centralLinkProtocol,
  centralStorage,
  claudeCodePeerMessaging,
  codexAcpAdapter,
  codexNativeIntegration,
  codexRouterAuth,
  codexRouterCli,
  codexRouterCore,
  codexRouterDebugHosts,
  codexRouterHost1,
  codexRouterHost2,
  codexRouterProxy2,
  codexRouterProxy,
  codexRouterQuotaResetTestHarness,
  codexRouterQuotaService,
  codexRouterQuota,
  codexRouterSecretStore,
  codexRouterSelection,
  codexRouterState,
  codexRouterTestSupport,
  collaborationApplicationTestSupport,
  collaborationCentralAdminProtocol,
  collaborationCentralCloudflare,
  collaborationCentralConformance,
  collaborationCentralCore1,
  collaborationCentralCore2,
  collaborationCentralCore3,
  collaborationCentralCore4,
  collaborationCentralCore5,
  collaborationCentralTokio,
  collaborationClient,
  collaborationContracts,
  collaborationMailProtocol,
  collaborationMcp,
  collaborationProtocol,
  collaborationRuntime1,
  collaborationRuntime2,
  collaborationService1,
  collaborationService2,
  collaborationService3,
  collaborationService4,
  fleetAdmission,
  fleetIdentityProtocol,
  interactionStorage,
  lifecycleObservation,
  messageBoardStorage1,
  messageBoardStorage2,
  messageBoard,
  nodeTestFixture,
  ownedProcessTestSupport,
  providersClaudeAcp,
  routerClock,
  sessionEventModel,
  sqliteStorageMaintenance,
  sqlxDurableObjectCore,
  sqlxDurableObject,
  tailnetVerificationHost,
  uiPreferenceStore,
  workspaceArchitectureCheck,
  workspaceIds,
  workspaceModel,
  workspaceRules,
];
