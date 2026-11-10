import assert from "node:assert/strict";
import { test } from "node:test";
import { mkdtemp, readFile, lstat, rm } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { z } from "zod";
import { Receipt, ObservationOpen } from "../receipt.ts";
import { decodeAuthoredJson } from "../exact-json.ts";
import { sanitizedInstallationFailure, recordInstallationFailure } from "./installation-failure.ts";

test("private failure receipt keeps stage and known failure category without arbitrary values", async () => {
  const root = await mkdtemp(join(tmpdir(), "2026-11-failure-receipt-")),
    secret = "synthetic-secret-must-not-be-recorded";
  try {
    const missing = await readFile(`${root}/${secret}`).catch((error: unknown) => error);
    assert.deepEqual(sanitizedInstallationFailure("hostManifestRead", missing), {
      stage: "hostManifestRead",
      cause: { kind: "filesystem", code: "ENOENT" },
    });
    const invalid = z.object({ version: z.literal(3) }).safeParse({ version: secret });
    assert.equal(invalid.success, false);
    if (invalid.success) throw new Error("invalid shape fixture was accepted");
    assert.deepEqual(sanitizedInstallationFailure("hostManifestShape", invalid.error), {
      stage: "hostManifestShape",
      cause: { kind: "invalidShape", issues: 1 },
    });
    const receipt = new Receipt(root);
    for (const error of [
      missing,
      invalid.error,
      new Error(secret, { cause: { kind: "httpRefusal", status: 403, body: secret } }),
      new Error(secret, { cause: { kind: "protocolRefusal", error: { token: secret } } }),
      new Error(secret, { cause: { kind: "refused", value: secret } }),
      new TypeError(secret),
      new AggregateError([new Error(secret)], secret),
      new ObservationOpen(secret),
      secret,
    ])
      await recordInstallationFailure(receipt, "continuationController", error);
    const raw = await readFile(`${root}/observations.jsonl`, "utf8");
    assert.equal(raw.includes(secret), false);
    assert.equal((await lstat(`${root}/observations.jsonl`)).mode & 0o777, 0o600);
    const rows = raw
      .trim()
      .split("\n")
      .map((line) => decodeAuthoredJson(line));
    assert.equal(rows.length, 9);
    assert.ok(
      rows.every(
        (row) =>
          z
            .object({
              kind: z.literal("installationFailure"),
              value: z.object({ stage: z.literal("continuationController") }),
            })
            .safeParse(row).success,
      ),
    );
  } finally {
    await rm(root, { recursive: true });
  }
});
test("readiness classification distinguishes MCP transport/protocol/HTTP and native availability", () => {
  for (const reason of ["transport", "invalidResponse"] as const)
    assert.deepEqual(
      sanitizedInstallationFailure(
        "hostEndpointRead",
        new ObservationOpen(`MCP endpoints_list ${reason}; effect unknown, no replay`),
      ),
      { stage: "hostEndpointRead", cause: { kind: "mcpUnconfirmed", reason } },
    );
  assert.deepEqual(
    sanitizedInstallationFailure(
      "hostNativeAvailability",
      new Error("native endpoint not available yet"),
    ),
    { stage: "hostNativeAvailability", cause: { kind: "nativeUnavailable" } },
  );
  assert.deepEqual(
    sanitizedInstallationFailure(
      "hostEndpointRead",
      new Error("discard", { cause: { kind: "httpRefusal", status: 403, body: "discard" } }),
    ),
    { stage: "hostEndpointRead", cause: { kind: "mcpHttpRefusal", status: 403 } },
  );
});
