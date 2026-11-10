import { createHash } from "node:crypto";
import { accessSync, constants, readFileSync, realpathSync, statSync } from "node:fs";
import { delimiter, isAbsolute, join, sep } from "node:path";
import { spawnSync } from "node:child_process";
import { binaryQualificationSchema, type QualifiedBinary } from "../shared/evidence-models.ts";
import { decode } from "../../exact-json.ts";

export function sha256(path: string): string {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

function readDebugSignature(path: string): string {
  const verified = spawnSync("/usr/bin/codesign", ["--verify", "--strict", path]);
  if (verified.error !== undefined) throw verified.error;
  if (verified.status !== 0) throw new Error("debug code signature verification failed");
  const signature = spawnSync("/usr/bin/codesign", ["-dv", "--verbose=4", path], {
    encoding: "utf8",
  });
  if (signature.error !== undefined) throw signature.error;
  if (signature.status !== 0) throw new Error("debug signature details unavailable");
  return signature.stderr;
}

function requireSigningIdentity(
  path: string,
  identifier: string,
  signatureProbe: (path: string) => string,
): void {
  const fields = signatureProbe(path).split("\n");
  if (!fields.includes(`Identifier=${identifier}`) || !fields.includes("TeamIdentifier=974QD84WVC"))
    throw new Error("debug signing identity mismatch");
}

export function qualifyNormalBinaries(
  qualification: string,
  signatureProbe: (path: string) => string = readDebugSignature,
  requiredTargets: readonly ("codex-router" | "agent-collaboration" | "agent-sessions")[] = [
    "codex-router",
    "agent-collaboration",
    "agent-sessions",
  ],
): Map<string, QualifiedBinary> {
  const records = binaryQualificationSchema.parse(decode(readFileSync(qualification, "utf8")));
  const binaries = new Map(records.binaries.map((record) => [record.target, record]));
  const verified = new Map<string, QualifiedBinary>();
  for (const name of requiredTargets) {
    const item = binaries.get(name);
    if (item === undefined) throw new Error(`qualified binary missing: ${name}`);
    if (sha256(item.path) !== item.sha256 || item.testGuardFeatures.length > 0)
      throw new Error("binary hash or normal-build mismatch");
    requireSigningIdentity(item.path, `dev.shravansunder.${name}.debug`, signatureProbe);
    verified.set(name, item);
  }
  return verified;
}

/** The declared proof example carries its real Cargo feature list, unlike a product binary. */
export function qualifyWorkerHost(input: {
  readonly qualification: string;
  readonly repository: string;
  readonly signatureProbe?: (path: string) => string;
}): QualifiedBinary {
  const records = binaryQualificationSchema.parse(
    decode(readFileSync(input.qualification, "utf8")),
  );
  const host = records.binaries.find((item) => item.target === "provider-only-debug-host");
  if (host === undefined) throw new Error("qualified provider-only proof Host missing");
  const expected = join(
    realpathSync(input.repository),
    "target/debug/examples/provider-only-debug-host",
  );
  if (realpathSync(host.path) !== expected || sha256(host.path) !== host.sha256)
    throw new Error("proof Host source/build path or hash mismatch");
  requireSigningIdentity(
    host.path,
    "dev.shravansunder.test.provider-only-debug-host",
    input.signatureProbe ?? readDebugSignature,
  );
  return host;
}

export function packageSearchPath(path: string): boolean {
  return path.split(sep).some((part) => ["node_modules", "_npx", "dlx"].includes(part));
}
export function excludedClaudeTarget(path: string): boolean {
  return path
    .split(sep)
    .some(
      (part) =>
        ["_npx", "dlx"].includes(part) ||
        part.startsWith("claude-agent-sdk") ||
        part.startsWith("@anthropic-ai+claude-agent-sdk"),
    );
}

export function resolveGlobalClaude(searchPath: string | null | undefined): string {
  const filename = process.platform === "win32" ? "claude.exe" : "claude";
  for (const entry of (searchPath ?? "").split(delimiter)) {
    if (!isAbsolute(entry) || packageSearchPath(entry)) continue;
    try {
      const directory = realpathSync(entry);
      if (packageSearchPath(directory)) continue;
      const candidate = realpathSync(join(directory, filename));
      if (excludedClaudeTarget(candidate) || !statSync(candidate).isFile()) continue;
      if (process.platform !== "win32") accessSync(candidate, constants.X_OK);
      return candidate;
    } catch {
      continue;
    }
  }
  throw new Error("qualified global Claude executable unavailable on Host PATH");
}

export function qualifyGlobalClaude(
  planned: string,
  searchPath: string | null | undefined,
): { readonly canonicalPath: string; readonly sha256: string } {
  const actual = resolveGlobalClaude(searchPath);
  if (realpathSync(planned) !== actual)
    throw new Error("planned Claude executable differs from qualified Host PATH selection");
  return { canonicalPath: actual, sha256: sha256(actual) };
}
