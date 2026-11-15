import assert from "node:assert/strict";
import { test } from "node:test";
import {
  endpointModeMetadata,
  observationMetadata,
  promptReceiptMetadata,
  providerMetadataSchema,
} from "./capture.ts";

const target = {
  endpoint: { serviceId: "service-a", endpointId: "claude-local" },
  sessionId: "owned-session",
};

test("public observation capture keeps correlation and tool status without model thoughts", (): void => {
  const observation = {
    target,
    generation: { serviceEpoch: "epoch", generation: 1n },
    attached: true,
    continuationGap: true,
    endReason: "resultLimitReached",
    events: [
      { sequence: 1n, event: { kind: "turnStarted", turnId: "turn-a", inputId: "input-a" } },
      {
        sequence: 2n,
        event: {
          kind: "itemUpdated",
          item: {
            itemId: "item-a",
            text: "PRIVATE_THOUGHT",
            kind: { kind: "reasoning", text: "PRIVATE_THOUGHT" },
          },
        },
      },
      {
        sequence: 3n,
        event: {
          kind: "itemUpdated",
          item: {
            itemId: "tool-a",
            text: "unneeded body",
            kind: { kind: "toolCall", status: "completed" },
          },
        },
      },
      { kind: "resyncRequired" },
      {
        sequence: 4n,
        event: {
          kind: "interactionRequested",
          interaction: {
            kind: "approval",
            request: {
              requestId: "approval-a",
              description: "unneeded presentation",
              subject: { toolCall: { toolCallId: "tool-a", kind: "execute" } },
            },
          },
        },
      },
    ],
  };
  const captured = observationMetadata(observation, target);
  const serialized = JSON.stringify(captured, (_key, value: unknown): unknown =>
    typeof value === "bigint" ? value.toString() : value,
  );
  assert.equal(serialized.includes("PRIVATE_THOUGHT"), false);
  assert.equal(serialized.includes("unneeded body"), false);
  assert.match(serialized, /turn-a/);
  assert.match(serialized, /tool-a/);
  assert.match(serialized, /completed/);
  // A truncated/gapped page remains visible, never silently labelled complete.
  assert.match(serialized, /resultLimitReached/);
  assert.match(serialized, /"continuationGap":true/);
  assert.match(serialized, /resyncRequired/);
  assert.match(serialized, /approval-a/);
  assert.equal(serialized.includes("unneeded presentation"), false);
});

test("a different target cannot masquerade as this session's observation", (): void => {
  assert.throws(
    (): unknown =>
      observationMetadata(
        {
          target: { ...target, sessionId: "another-session" },
          events: [],
          attached: true,
          continuationGap: true,
          endReason: "deadlineReached",
        },
        target,
      ),
    /target mismatch/,
  );
});

test("pending prompt identity survives without retaining notification thoughts", (): void => {
  const receipt = promptReceiptMetadata({
    kind: "answered",
    value: { kind: "pending", operationId: "actual-operation", target },
    notifications: [
      { method: "observation", params: { text: "PRIVATE_THOUGHT" }, receivedAfterMs: 0 },
    ],
  });
  const serialized = JSON.stringify(receipt);
  assert.match(serialized, /actual-operation/);
  assert.match(serialized, /pending/);
  assert.equal(serialized.includes("PRIVATE_THOUGHT"), false);
});

test("mode capture preserves requested, provider actual and fallback reason without an auth label", (): void => {
  const observed = providerMetadataSchema.parse({
    target,
    state: "idle",
    requestedMode: "auto",
    modeReason: "escalationUnproven",
    capabilities: { authStatus: { kind: "account", label: "PRIVATE_ACCOUNT_LABEL" } },
    settingsCatalog: { currentMode: "acceptEdits", configOptions: [] },
  });
  assert.equal(observed.requestedMode, "auto");
  assert.equal(observed.settingsCatalog.currentMode, "acceptEdits");
  assert.equal(observed.modeReason, "escalationUnproven");
  assert.equal(JSON.stringify(observed).includes("PRIVATE_ACCOUNT_LABEL"), false);
});

test("R7 capture selects the exact endpoint and never replaces an unknown realization", (): void => {
  const entry = {
    endpoint: target.endpoint,
    availability: { state: "available" },
    channels: [
      { kind: "externalProvider", path: "PRIVATE_SOCKET_PATH", autoRealization: "unknown" },
    ],
  };
  const observed = endpointModeMetadata(
    {
      endpoints: [
        {
          ...entry,
          endpoint: { ...target.endpoint, serviceId: "another-service" },
          channels: [{ kind: "externalProvider", autoRealization: "classifierEscalates" }],
        },
        entry,
      ],
    },
    target.endpoint,
  );
  assert.equal(observed.channels[0]?.autoRealization, "unknown");
  assert.equal(JSON.stringify(observed).includes("PRIVATE_SOCKET_PATH"), false);
  assert.throws(
    (): unknown => endpointModeMetadata({ endpoints: [entry, entry] }, target.endpoint),
    /exactly/,
  );
  assert.throws((): unknown => endpointModeMetadata({ endpoints: [] }, target.endpoint), /exactly/);
});
