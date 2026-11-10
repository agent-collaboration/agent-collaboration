import assert from "node:assert/strict";
import { test } from "node:test";
import { oldDirectoryEntry } from "./old-central-registry.ts";

test("old2636 Registry preserves the snake-case expiry, installation holder and full lease-version tuple", (): void => {
  const raw = {
    project: "01900000-0000-7000-8000-000000000001",
    name: "old wire",
    workspace: "01900000-0000-7000-8000-000000000002",
    board: "01900000-0000-7000-8000-000000000003",
    boardArchive: "open",
    copySide: { kind: "notYetReported" },
    leaseSide: {
      kind: "known",
      value: {
        lease: {
          kind: "held",
          value: {
            epoch: 2n,
            holder: "0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89",
            state: "held",
            expires_at: "2026-11-10T05:35:55.013Z",
          },
        },
        leaseVersion: { epoch: 2n, incarnation: 3n, change: 8n },
        asOf: "2026-11-10T01:35:55.013Z",
      },
    },
  };
  assert.deepEqual(oldDirectoryEntry.parse(raw), raw);
  assert.throws(() =>
    oldDirectoryEntry.parse({
      ...raw,
      leaseSide: { ...raw.leaseSide, value: { ...raw.leaseSide.value, leaseVersion: 8n } },
    }),
  );
  const { expires_at: expiry, ...fields } = raw.leaseSide.value.lease.value;
  assert.throws(() =>
    oldDirectoryEntry.parse({
      ...raw,
      leaseSide: {
        ...raw.leaseSide,
        value: {
          ...raw.leaseSide.value,
          lease: { kind: "held", value: { ...fields, expiresAt: expiry } },
        },
      },
    }),
  );
});
