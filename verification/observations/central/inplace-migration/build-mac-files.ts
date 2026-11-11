/** Assemble migrate's strict macFiles body from passive, snapshot and scratch-window readbacks. */
import { createReadStream } from "node:fs";
import { lstat, readFile, realpath, writeFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { createInterface } from "node:readline";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { z } from "zod";
import { decode, decodeAuthoredJson, encode } from "../../../exact-json.ts";

const unsigned = z.bigint().nonnegative().max(18_446_744_073_709_551_615n);
const position = z.object({ epoch: unsigned.positive(), sequence: unsigned }).strict();
const identity = z.uuidv7();
const hash = z.string().regex(/^[a-f0-9]{64}$/);
const entry = z
  .object({
    database: identity,
    passive: z.string(),
    snapshot: z.string(),
    inspection: z.string(),
    bSha256: hash,
  })
  .strict();
const plan = z
  .object({
    expectedFiles: z.number().int().positive(),
    output: z.string(),
    entries: z.array(entry).min(1),
  })
  .strict();
async function checkPrivateInput(path: string): Promise<void> {
  const info = await lstat(path);
  if (
    !info.isFile() ||
    info.isSymbolicLink() ||
    info.uid !== process.getuid?.() ||
    (info.mode & 0o077) !== 0 ||
    (await realpath(path)) !== path
  )
    throw new Error("canonical private input required");
}
async function privateInput(path: string): Promise<string> {
  await checkPrivateInput(path);
  return readFile(path, "utf8");
}
async function selectedFrames(path: string) {
  await checkPrivateInput(path);
  let header: unknown, complete: unknown;
  for await (const line of createInterface({
    input: createReadStream(path),
    crlfDelay: Infinity,
  })) {
    if (line.includes('"kind":"seedSnapshotHeader"')) {
      if (header !== undefined) throw new Error("duplicate header");
      header = decode(line);
    }
    if (line.includes('"kind":"seedSnapshotComplete"')) {
      if (complete !== undefined) throw new Error("duplicate completion");
      complete = decode(line);
    }
  }
  return { header, complete };
}
const before = (a: z.infer<typeof position>, b: z.infer<typeof position>): boolean =>
  a.epoch < b.epoch || (a.epoch === b.epoch && a.sequence < b.sequence);
const passiveSchema = z
  .object({
    kind: z.literal("retainedWriterObserved"),
    database: identity,
    holder: identity,
    lease: unsigned.positive(),
    originEpoch: unsigned.positive(),
    retainedAuthority: z.literal("noObservedLoss"),
    currentRenewalObserved: z.literal(false),
  })
  .passthrough();
const headerSchema = z
  .object({
    kind: z.literal("seedSnapshotHeader"),
    database: identity,
    holder: identity,
    position,
    copies: z.array(
      z
        .object({
          copy: identity,
          role: z.enum(["authoritative", "follower"]),
          start: position,
          acked: position,
          needsSnapshot: z.boolean(),
        })
        .strict(),
    ),
    registration: z
      .object({
        Ready: z
          .object({
            lease: z
              .object({ database: identity, holder: identity, epoch: unsigned.positive() })
              .passthrough(),
          })
          .strict(),
      })
      .strict(),
  })
  .passthrough();
const doneSchema = z
  .object({
    kind: z.literal("seedSnapshotComplete"),
    database: identity,
    holder: identity,
    position,
    untouchedBeforeSha256: hash,
    untouchedAfterSha256: hash,
    scratchAfterSha256: hash,
    currentRenewalObserved: z.literal(false),
  })
  .passthrough();
const inspectedSchema = z
  .object({
    observation: z
      .object({
        kind: z.literal("ledgerInspection"),
        window: z.object({ retainedAfterRaw: z.string() }).passthrough(),
      })
      .passthrough(),
    scratchBeforeSha256: hash,
    scratchAfterSha256: hash,
    untouchedBeforeSha256: hash,
    untouchedAfterSha256: hash,
  })
  .strict();

async function facts(input: z.infer<typeof entry>) {
  const passive = passiveSchema.parse(decode(await privateInput(input.passive)));
  const frames = await selectedFrames(input.snapshot);
  const header = headerSchema.parse(frames.header);
  const done = doneSchema.parse(frames.complete);
  const inspected = inspectedSchema.parse(decode(await privateInput(input.inspection)));
  const floor = position.parse(decode(inspected.observation.window.retainedAfterRaw));
  const authoritative = header.copies.filter((copy) => copy.role === "authoritative");
  const [sender] = authoritative;
  const lease = header.registration.Ready.lease;
  if (
    passive.database !== input.database ||
    header.database !== input.database ||
    done.database !== input.database ||
    lease.database !== input.database ||
    header.holder !== passive.holder ||
    done.holder !== passive.holder ||
    lease.holder !== passive.holder ||
    lease.epoch !== passive.lease ||
    passive.originEpoch !== passive.lease ||
    before(header.position, floor) ||
    header.position.epoch > passive.lease ||
    encode(done.position) !== encode(header.position) ||
    authoritative.length !== 1 ||
    sender === undefined ||
    sender.needsSnapshot ||
    before(sender.acked, floor) ||
    before(header.position, sender.acked) ||
    before(sender.acked, sender.start) ||
    done.untouchedBeforeSha256 !== input.bSha256 ||
    done.untouchedAfterSha256 !== input.bSha256 ||
    inspected.untouchedBeforeSha256 !== input.bSha256 ||
    inspected.untouchedAfterSha256 !== input.bSha256 ||
    inspected.scratchBeforeSha256 !== inspected.scratchAfterSha256 ||
    inspected.scratchBeforeSha256 !== done.scratchAfterSha256
  )
    throw new Error("source identity, lease, sender, window or same-file provenance mismatch");
  return {
    database: input.database,
    holder: passive.holder,
    lease: passive.lease,
    senderReceiver: sender.copy,
    retained: { floor, head: header.position },
  };
}
export async function buildMacFiles(planPath: string) {
  const input = plan.parse(decodeAuthoredJson(await privateInput(planPath)));
  const parent = dirname(input.output);
  const directory = await lstat(parent);
  if (
    !input.output.startsWith("/") ||
    !directory.isDirectory() ||
    directory.uid !== process.getuid?.() ||
    (directory.mode & 0o077) !== 0 ||
    (await realpath(parent)) !== parent
  )
    throw new Error("canonical owner-private output directory required");
  if (
    input.entries.length !== input.expectedFiles ||
    new Set(input.entries.map((row) => row.database)).size !== input.expectedFiles
  )
    throw new Error("missing or duplicate projects");
  const macFiles = [];
  const sources = [];
  for (const item of input.entries) {
    macFiles.push(await facts(item));
    const files = [];
    for (const path of [item.passive, item.snapshot, item.inspection]) {
      const bytes = await readFile(path);
      files.push({ path, sha256: createHash("sha256").update(bytes).digest("hex") });
    }
    sources.push({ database: item.database, files });
  }
  const body = encode({ macFiles });
  await writeFile(input.output, body, { mode: 0o600, flag: "wx" });
  await writeFile(
    input.output + ".provenance.json",
    encode({
      sources,
      floorSource: "_fleet_sync_writer.record -> '$.retainedAfter'",
      headSource:
        "WriterSnapshot.position() via capture_stream::capture_head; no stored head column",
      noCurrentRenewalClaim: true,
      staleIfLiveWriterChanges: true,
    }),
    { mode: 0o600, flag: "wx" },
  );
  return {
    output: input.output,
    files: macFiles.length,
    sha256: createHash("sha256").update(body).digest("hex"),
  };
}
if (process.argv[1] !== undefined && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const path = process.argv[2];
  if (process.argv.length !== 3 || path === undefined || path === "--help") {
    console.log(
      "build-mac-files.ts PRIVATE_PLAN\nPlan: {expectedFiles,output,entries:[{database,passive,snapshot,inspection,bSha256}]}. No network, writes only new private output/provenance.",
    );
    process.exitCode = path === "--help" ? 0 : 2;
  } else console.log(encode(await buildMacFiles(path)));
}
