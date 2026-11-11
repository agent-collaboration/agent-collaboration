/** Launch the signed verification-only retained reader against one declared sealed backup. */
import { mkdir, writeFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { join } from "node:path";
import { z } from "zod";
import { type Artifact } from "../../../declaration.ts";
import { decode } from "../../../exact-json.ts";
import {
  command,
  qualifyArtifact,
  fingerprint,
  privateDirectory,
} from "../../../machines/owned-process.ts";
import { identifier, unsigned, type Source } from "./contracts.ts";

const observedWriter = z
  .object({
    kind: z.literal("retainedWriterObserved"),
    database: identifier,
    holder: identifier,
    lease: unsigned,
    originEpoch: unsigned,
    originStart: z.object({ epoch: unsigned, sequence: unsigned }).strict(),
    originalRegistration: z
      .object({ database: identifier, holder: identifier, epoch: unsigned })
      .passthrough(),
    retainedAuthority: z.literal("noObservedLoss"),
    sourceSha256: z.string().regex(/^[a-f0-9]{64}$/),
    currentRenewalObserved: z.literal(false),
  })
  .strict();
export function validateRetainedWriter(input: {
  raw: unknown;
  expectedProject: string;
  expectedMachine: string;
  sourceSha256: string;
}): { database: string; holder: string; lease: bigint } {
  const observed = observedWriter.parse(input.raw);
  if (
    observed.database !== input.expectedProject ||
    observed.holder !== input.expectedMachine ||
    observed.sourceSha256 !== input.sourceSha256 ||
    observed.originalRegistration.database !== observed.database ||
    observed.originalRegistration.holder !== observed.holder ||
    observed.originalRegistration.epoch !== observed.lease ||
    observed.originEpoch !== observed.lease
  )
    throw new Error("retained writer identity, lease, origin or source mismatch");
  return { database: observed.database, holder: observed.holder, lease: observed.lease };
}
export async function observeRetainedWriter(input: {
  artifact: Artifact;
  backup: { path: string; sha256: string };
  expectedProject: string;
  expectedMachine: string;
  evidenceDirectory: string;
}): Promise<{ writer: ReturnType<typeof validateRetainedWriter>; source: Source }> {
  identifier.parse(input.expectedProject);
  identifier.parse(input.expectedMachine);
  await qualifyArtifact(input.artifact);
  if ((await fingerprint(input.backup.path)) !== input.backup.sha256)
    throw new Error("sealed backup hash differs");
  await mkdir(input.evidenceDirectory, { mode: 0o700 });
  await privateDirectory(input.evidenceDirectory);
  const outcome = await command(input.artifact.path, [input.backup.path, input.expectedProject]);
  if (outcome.exit !== 0)
    throw new Error("retained reader refused; inspect sealed input, do not substitute SQL");
  const writer = validateRetainedWriter({
    raw: decode(outcome.stdout),
    expectedProject: input.expectedProject,
    expectedMachine: input.expectedMachine,
    sourceSha256: input.backup.sha256,
  });
  if ((await fingerprint(input.backup.path)) !== input.backup.sha256)
    throw new Error("sealed backup changed");
  await qualifyArtifact(input.artifact);
  const artifact = join(input.evidenceDirectory, "retained-writer.json");
  await writeFile(artifact, outcome.stdout, { mode: 0o600, flag: "wx" });
  return {
    writer,
    source: {
      artifact,
      sha256: createHash("sha256").update(outcome.stdout).digest("hex"),
      method: "retainedWriterRead",
    },
  };
}
