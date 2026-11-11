/** GET-only authoritative Registry/Copy facts for the commissioned eight-project Sunbook cohort. */
import { createHash } from "node:crypto";
import { writeFile } from "node:fs/promises";
import { z } from "zod";
import { decode, decodeAuthoredJson, encode } from "../../../exact-json.ts";
import { registerCommandRoot } from "../../../machines/command-ledger.ts";
import { command, privateDirectory } from "../../../machines/owned-process.ts";
import { healthReply, type Source } from "./contracts.ts";
import { directoryEntry, directoryPage } from "./contracts.ts";
import { readPrivateText, type CentralRead } from "./record.ts";
import { stagingRoot, stagingOrigin, syntheticReceipts } from "./s1-synthetic-cli.ts";

const expectedProject = z.discriminatedUnion("expectation", [
  z
    .object({
      expectation: z.literal("held"),
      project: z.uuidv7(),
      holderInstallation: z.uuid(),
      holderPeer: z.uuidv7(),
      lease: z.bigint().positive(),
    })
    .strict(),
  z
    .object({
      expectation: z.literal("lifecycle"),
      project: z.uuidv7(),
      holderInstallation: z.uuid(),
      holderPeer: z.uuidv7(),
    })
    .strict(),
]);
type ExpectedProject = z.infer<typeof expectedProject>;

export function matchRegistryCopy(input: {
  expected: ExpectedProject;
  entry: z.infer<typeof directoryEntry>;
  health: z.infer<typeof healthReply>;
}): void {
  const { expected, entry, health } = input;
  if (entry.project !== expected.project || entry.leaseSide.kind !== "known")
    throw new Error("requested project has no authoritative Registry lifecycle");
  const lease = entry.leaseSide.value.lease;
  if (
    expected.expectation === "held" &&
    (lease.kind !== "held" ||
      lease.value.holder !== expected.holderInstallation ||
      lease.value.epoch !== expected.lease)
  )
    throw new Error("Registry grant differs from the declared D/M/N");
  if (lease.kind === "retired") {
    if (health.kind !== "retired") throw new Error("retired Registry/Copy disagree");
    return;
  }
  if (
    health.kind !== "live" ||
    health.value.copy.kind !== "known" ||
    health.value.copy.value === null
  )
    throw new Error("live public Copy facts unavailable");
  const copy = health.value.copy.value;
  if (copy.database !== expected.project || copy.role !== "authoritative")
    throw new Error("Copy identity or authoritative role differs");
  if (
    lease.kind === "held" &&
    (copy.holder !== expected.holderPeer ||
      lease.value.holder !== expected.holderInstallation ||
      copy.fence !== lease.value.epoch)
  )
    throw new Error("Registry/Copy holder or fence differs");
  if (lease.kind === "unheld" && copy.holder !== null)
    throw new Error("unheld Registry has a held Copy");
}

export async function collectSelectedRegistry(input: {
  expected: readonly ExpectedProject[];
  read: CentralRead;
}): Promise<{
  directoryCount: number;
  sources: Source[];
  projects: {
    expected: ExpectedProject;
    directory: z.infer<typeof directoryEntry>;
    health: z.infer<typeof healthReply>;
    registryLease: unknown;
  }[];
}> {
  const expected = input.expected.map((value): ExpectedProject => expectedProject.parse(value));
  if (new Set(expected.map((entry): string => entry.project)).size !== expected.length)
    throw new Error("unique expected cohort required");
  const directory: z.infer<typeof directoryEntry>[] = [];
  const seen = new Set<string>(),
    sources: Source[] = [];
  let after: string | null = null;
  for (let page = 0; page < 1000; page += 1) {
    const observed = await input.read(
      `/admin/v1/copies?limit=100${after === null ? "" : `&after=${after}`}`,
    );
    const parsed = directoryPage.parse(observed.body);
    sources.push({ artifact: observed.artifact, sha256: observed.sha256, method: "centralGet" });
    for (const entry of parsed.projects) {
      if (seen.has(entry.project) || (after !== null && entry.project <= after))
        throw new Error("duplicate or nonadvancing Registry project");
      seen.add(entry.project);
      directory.push(entry);
    }
    if (parsed.nextAfter === null) {
      after = null;
      break;
    }
    if (parsed.projects.at(-1)?.project !== parsed.nextAfter)
      throw new Error("invalid Registry page continuation");
    after = parsed.nextAfter;
  }
  if (after !== null) throw new Error("Registry directory page bound exceeded");
  const projects = [];
  for (const requested of expected) {
    const entry = directory.find((record): boolean => record.project === requested.project);
    if (entry === undefined)
      throw new Error("expected project missing from complete Registry directory");
    const observed = await input.read(`/admin/v1/copies/${requested.project}/health`);
    const health = healthReply.parse(observed.body);
    sources.push({ artifact: observed.artifact, sha256: observed.sha256, method: "centralGet" });
    matchRegistryCopy({ expected: requested, entry, health });
    projects.push({
      expected: requested,
      directory: entry,
      health,
      registryLease:
        entry.leaseSide.kind === "known" ? entry.leaseSide.value.lease : { kind: "notYetReported" },
    });
  }
  return { directoryCount: directory.length, sources, projects };
}

async function expectations(): Promise<ExpectedProject[]> {
  const baseline = z
    .object({
      peerId: z.uuidv7(),
      installationId: z.uuid(),
      files: z
        .array(
          z.object({
            project: z.uuidv7(),
            holder: z.uuidv7(),
            lease: z.string().regex(/^[1-9][0-9]*$/),
          }),
        )
        .length(4),
    })
    .parse(
      decodeAuthoredJson(
        await readPrivateText("/private/tmp/2026-11-s1-staging/s7/sunbook-baseline-plan-3.json"),
      ),
    );
  const synthetic = z
    .object({
      ids: z.object({
        heldProject: z.uuidv7(),
        retiredProject: z.uuidv7(),
        archivedProject: z.uuidv7(),
      }),
    })
    .parse(decodeAuthoredJson(await readPrivateText(`${syntheticReceipts}/manifest.json`))).ids;
  const retirement = z
    .object({ project: z.uuidv7() })
    .parse(
      decodeAuthoredJson(await readPrivateText(`${syntheticReceipts}/retirement/manifest.json`)),
    );
  return [
    ...baseline.files.map((file): ExpectedProject => ({
      expectation: "held",
      project: file.project,
      holderInstallation: baseline.installationId,
      holderPeer: file.holder,
      lease: BigInt(file.lease),
    })),
    {
      expectation: "held",
      project: synthetic.heldProject,
      holderInstallation: baseline.installationId,
      holderPeer: baseline.peerId,
      lease: 1n,
    },
    {
      expectation: "held",
      project: synthetic.retiredProject,
      holderInstallation: baseline.installationId,
      holderPeer: baseline.peerId,
      lease: 1n,
    },
    {
      expectation: "lifecycle",
      project: synthetic.archivedProject,
      holderInstallation: baseline.installationId,
      holderPeer: baseline.peerId,
    },
    {
      expectation: "held",
      project: retirement.project,
      holderInstallation: baseline.installationId,
      holderPeer: baseline.peerId,
      lease: 1n,
    },
  ];
}

async function boundedBody(response: Response): Promise<Buffer> {
  if (response.body === null) throw new Error("empty central response");
  const reader = response.body.getReader(),
    chunks: Uint8Array[] = [];
  let bytes = 0;
  for (;;) {
    const chunk = await reader.read();
    if (chunk.done) return Buffer.concat(chunks);
    bytes += chunk.value.byteLength;
    if (bytes > 16 * 1024 * 1024) {
      await reader.cancel();
      throw new Error("central response exceeds bound");
    }
    chunks.push(chunk.value);
  }
}

async function verifyNetworkPreflight(directory: string): Promise<void> {
  const network = z
    .object({ exit: z.literal(0), stdout: z.string() })
    .parse(decodeAuthoredJson(await readPrivateText(`${directory}/network-reply.json`)));
  z.object({
    kind: z.literal("nodeNetworkObserved"),
    url: z.literal(`${stagingOrigin}/v1/health`),
    status: z.number().int(),
  }).parse(decodeAuthoredJson(network.stdout));
}

async function run(): Promise<void> {
  process.umask(0o077);
  const directory = `${syntheticReceipts}/registry`;
  await privateDirectory(directory);
  const release = await registerCommandRoot(stagingRoot);
  const save = async (name: string, value: unknown): Promise<void> => {
    await writeFile(`${directory}/${name}.json`, `${encode(value)}\n`, { mode: 0o600, flag: "wx" });
  };
  try {
    await verifyNetworkPreflight(directory);
    const expected = await expectations();
    await save("run", {
      expected,
      origin: stagingOrigin,
      method: "GET",
      sourceHead: "0cb050dd647a300a033195261315b1462a06ef39",
      centralDeploymentIndependentlyVerified: false,
    });
    await save("owner-access-request", {
      executable: "cloudflared",
      arguments: ["access", "token", `--app=${stagingOrigin}`],
      tokenPersisted: false,
    });
    const assertion = await command("cloudflared", ["access", "token", `--app=${stagingOrigin}`], {
      cwd: stagingRoot,
      timeoutSeconds: 60,
    });
    const token = assertion.stdout.trim();
    await save("owner-access-outcome", {
      exit: assertion.exit,
      assertionPresent: assertion.exit === 0 && token.split(".").length === 3,
      tokenPersisted: false,
    });
    if (assertion.exit !== 0 || token.split(".").length !== 3)
      throw new Error(
        "existing owner Access assertion unavailable; no authenticated read attempted",
      );
    let sequence = 0;
    const read: CentralRead = async (path): ReturnType<CentralRead> => {
      if (
        !/^\/admin\/v1\/copies(?:\?limit=100(?:&after=[a-f0-9-]+)?|\/[a-f0-9-]+\/health)$/.test(
          path,
        )
      )
        throw new Error("read route not allowed");
      const name = `central-${sequence++}`;
      await save(`${name}-request`, {
        origin: stagingOrigin,
        path,
        method: "GET",
        authentication: "owner Access assertion in memory",
        headerName: "cf-access-token",
      });
      // Same ingress authentication as tools/central-migration/scripts/owner_transport.ts.
      const response = await fetch(new URL(path, stagingOrigin), {
        method: "GET",
        headers: { "cf-access-token": token },
        redirect: "manual",
        signal: AbortSignal.timeout(30_000),
      });
      await save(`${name}-status`, { status: response.status, headersPersisted: false });
      if (response.status !== 200) {
        await response.body?.cancel();
        throw new Error(`central GET refused with HTTP ${response.status}`);
      }
      const body = await boundedBody(response),
        artifact = `${directory}/${name}-reply.json`;
      await writeFile(artifact, body, { mode: 0o600, flag: "wx" });
      return {
        body: decode(body.toString("utf8")),
        artifact,
        sha256: createHash("sha256").update(body).digest("hex"),
      };
    };
    const observed = await collectSelectedRegistry({ expected, read });
    await save("summary", {
      kind: "selectedRegistryCopiesObserved",
      capturedAt: new Date().toISOString(),
      origin: stagingOrigin,
      ...observed,
      atomicAcrossCalls: false,
    });
    process.stdout.write(
      `${encode({ kind: "selectedRegistryCopiesObserved", projects: observed.projects.length, directoryCount: observed.directoryCount, evidence: directory })}\n`,
    );
  } catch (error: unknown) {
    await save("incomplete", {
      message: error instanceof Error ? error.message : String(error),
      writesSent: 0,
    });
    throw error;
  } finally {
    release();
  }
}
if (import.meta.main)
  void run().catch((): void => {
    process.stderr.write("Registry readback incomplete; inspect private receipts\n");
    process.exitCode = 1;
  });
