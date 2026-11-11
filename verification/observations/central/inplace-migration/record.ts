import { createHash, randomUUIDv7 } from "node:crypto";
import { mkdir, lstat, readFile, writeFile } from "node:fs/promises";
import { resolve, join } from "node:path";
import { z } from "zod";
import { decode, encode } from "../../../exact-json.ts";
import {
  capturePlan,
  directoryPage,
  healthReply,
  routerObservation,
  snapshot,
  type CapturePlan,
  type Snapshot,
  type Source,
} from "./contracts.ts";

export interface ReadObservation {
  readonly body: unknown;
  readonly sha256: string;
  readonly artifact: string;
}
export type CentralRead = (path: string) => Promise<ReadObservation>;
const pageBound = 10_000;

export async function collectCentral(input: {
  plan: CapturePlan;
  read: CentralRead;
  routers?: readonly z.infer<typeof routerObservation>[];
}): Promise<Snapshot> {
  const directory: Snapshot["directory"] = [],
    copies: Snapshot["copies"] = [],
    sources: Source[] = [];
  const ids = new Set<string>(),
    cursors = new Set<string>();
  let after: string | null = null;
  for (let page = 0; ; page += 1) {
    if (page >= pageBound) throw new Error("directory page bound exceeded; capture incomplete");
    const path = `/admin/v1/copies?limit=100${after === null ? "" : `&after=${after}`}`;
    const observed = await input.read(path),
      parsed = directoryPage.parse(observed.body);
    sources.push({ artifact: observed.artifact, sha256: observed.sha256, method: "centralGet" });
    for (const entry of parsed.projects) {
      if (ids.has(entry.project) || (after !== null && entry.project <= after))
        throw new Error("duplicate or nonadvancing directory project");
      ids.add(entry.project);
      directory.push(entry);
    }
    if (parsed.nextAfter === null) break;
    if (cursors.has(parsed.nextAfter) || parsed.projects.at(-1)?.project !== parsed.nextAfter)
      throw new Error("invalid directory cursor");
    cursors.add(parsed.nextAfter);
    after = parsed.nextAfter;
  }
  if (directory.length === 0) throw new Error("empty central directory cannot prove preservation");
  for (const entry of directory) {
    const observed = await input.read(`/admin/v1/copies/${entry.project}/health`);
    const health = healthReply.parse(observed.body);
    if (
      health.kind === "live" &&
      health.value.copy.kind === "known" &&
      health.value.copy.value !== null &&
      health.value.copy.value.database !== entry.project
    )
      throw new Error("health database differs from requested project");
    copies.push({ project: entry.project, health });
    sources.push({ artifact: observed.artifact, sha256: observed.sha256, method: "centralGet" });
  }
  return snapshot.parse({
    format: "central-inplace-observation-v1",
    runId: randomUUIDv7(),
    central: { origin: input.plan.centralOrigin, sourceHead: input.plan.sourceHead },
    phase: input.plan.phase,
    routerTarget: input.plan.routerTarget,
    routerProjects: input.plan.routerProjects,
    capturedAt: new Date().toISOString(),
    directory,
    copies,
    sources,
    routers: input.routers ?? [],
    limitations: [
      "Registry-only and orphan objects have no independent enumeration route; separate inventory/decoder/S4 second line required.",
      "projectFingerprint hashes schema, not row content; complete non-holder public content reads are required separately.",
      "HTTP read calls can enter existing object handlers; this tool sends GET only, not a promise of storage-level passivity.",
      "Capture must be made at a quiescent cut; no atomic transaction spans HTTP pages and Routers.",
    ],
  });
}
export async function readPrivateText(path: string): Promise<string> {
  const info = await lstat(path);
  if (
    !info.isFile() ||
    info.isSymbolicLink() ||
    info.uid !== process.getuid?.() ||
    (info.mode & 0o077) !== 0 ||
    info.size > 32 * 1024 * 1024
  )
    throw new Error("bounded owner-private regular JSON file required");
  return readFile(path, "utf8");
}
export async function readPrivateJson(path: string): Promise<unknown> {
  return decode(await readPrivateText(path));
}
export async function verifyObservationSources(observed: Snapshot): Promise<void> {
  const sources = [
    ...observed.sources,
    ...observed.routers.flatMap((router): Source[] => [
      ...router.sources,
      ...router.content.map((content): Source => content.source),
      ...(router.authentication.kind === "observed" ? [router.authentication.source] : []),
    ]),
  ];
  for (const source of sources) {
    await readPrivateJson(source.artifact);
    if (
      createHash("sha256")
        .update(await readFile(source.artifact))
        .digest("hex") !== source.sha256
    )
      throw new Error("source artifact fingerprint mismatch");
  }
}

export async function recordSnapshot(rawPlan: unknown): Promise<string> {
  const plan = capturePlan.parse(rawPlan),
    origin = new URL(plan.centralOrigin);
  if (
    origin.username !== "" ||
    origin.password !== "" ||
    origin.search !== "" ||
    origin.hash !== "" ||
    origin.pathname !== "/" ||
    !(
      origin.protocol === "https:" ||
      (origin.protocol === "http:" && origin.hostname === "127.0.0.1")
    )
  )
    throw new Error("explicit HTTPS central origin or loopback fixture required");
  const token = z.string().min(1).parse(process.env[plan.ownerAssertionEnv]);
  const evidence = resolve(plan.evidenceDirectory);
  if (evidence !== plan.evidenceDirectory) throw new Error("canonical evidence path required");
  await mkdir(evidence, { mode: 0o700 });
  const info = await lstat(evidence);
  if (
    !info.isDirectory() ||
    info.isSymbolicLink() ||
    info.uid !== process.getuid?.() ||
    (info.mode & 0o077) !== 0
  )
    throw new Error("owner-private evidence directory required");
  let sequence = 0;
  const read: CentralRead = async (path): Promise<ReadObservation> => {
    if (
      !/^\/admin\/v1\/copies(?:\?limit=100(?:&after=[a-f0-9-]+)?|\/[a-f0-9-]+\/health)$/.test(path)
    )
      throw new Error("read path not allowlisted");
    const response = await fetch(new URL(path, origin), {
      method: "GET",
      headers: { "Cf-Access-Jwt-Assertion": token },
      redirect: "error",
      signal: AbortSignal.timeout(30_000),
    });
    if (response.status !== 200)
      throw new Error(`independent central GET refused: ${response.status}`);
    if (response.body === null) throw new Error("empty HTTP response");
    const reader = response.body.getReader();
    const chunks: Uint8Array[] = [];
    let bytes = 0;
    for (;;) {
      const chunk = await reader.read();
      if (chunk.done) break;
      bytes += chunk.value.byteLength;
      if (bytes > 16 * 1024 * 1024) {
        await reader.cancel();
        throw new Error("HTTP body exceeds bound");
      }
      chunks.push(chunk.value);
    }
    const body = Buffer.concat(chunks),
      artifact = join(evidence, `central-${sequence++}.json`);
    const parsed = decode(body.toString("utf8"));
    if (path.endsWith("/health")) healthReply.parse(parsed);
    else directoryPage.parse(parsed);
    await writeFile(artifact, body, { mode: 0o600, flag: "wx" });
    return { body: parsed, artifact, sha256: createHash("sha256").update(body).digest("hex") };
  };
  const routers = [];
  for (const path of plan.routerObservationFiles)
    routers.push(routerObservation.parse(await readPrivateJson(path)));
  const observed = await collectCentral({ plan, read, routers });
  await verifyObservationSources(observed);
  const output = join(evidence, "snapshot.json");
  await writeFile(output, `${encode(observed)}\n`, { mode: 0o600, flag: "wx" });
  return output;
}
