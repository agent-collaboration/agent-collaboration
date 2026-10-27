import {
  appendFileSync,
  chmodSync,
  existsSync,
  readFileSync,
  realpathSync,
  statSync,
} from "node:fs";
import { join } from "node:path";
import { isDeepStrictEqual } from "node:util";
import { createHash } from "node:crypto";
import { parse } from "smol-toml";
import { z } from "zod";
import { canonicalPath } from "../filesystem-paths.ts";
import { decode, objectSchema } from "../exact-json.ts";
import { evidenceJson } from "../evidence-json.ts";
import {
  allowedSectionChange,
  compareSections,
  fingerprintSections,
  sectionChangeSchema,
  sectionFingerprintSchema,
} from "./config-sections.ts";
import { exactIntegerSchema } from "./evidence-models.ts";

export class ConfigurationChanged extends Error {
  constructor(message: string) {
    super(message);
    this.name = "ConfigurationChanged";
  }
}

export const fileFingerprintSchema = z.strictObject({
  path: z.string(),
  sha256: z.string(),
  size: exactIntegerSchema,
  modified_ns: exactIntegerSchema,
});
export type FileFingerprint = z.infer<typeof fileFingerprintSchema>;
export const codexHomeSnapshotSchema = z.strictObject({
  profile: fileFingerprintSchema,
  config: fileFingerprintSchema,
  config_sections: z.array(sectionFingerprintSchema),
});
export type CodexHomeSnapshot = z.infer<typeof codexHomeSnapshotSchema>;
export const configurationObservedSchema = z.strictObject({
  kind: z.literal("observed").default("observed"),
  before: codexHomeSnapshotSchema,
  after: codexHomeSnapshotSchema,
  config_changes: z.array(sectionChangeSchema),
  writer_attribution: z.literal("unattributed").default("unattributed"),
});
export const configurationUnreadableSchema = z.strictObject({
  kind: z.literal("unreadable").default("unreadable"),
  before: codexHomeSnapshotSchema,
  error: z.string(),
});
export const configurationObservationSchema = z.discriminatedUnion("kind", [
  configurationObservedSchema,
  configurationUnreadableSchema,
]);
export type ConfigurationObservation = z.infer<typeof configurationObservationSchema>;

export function configurationUnchanged(observation: ConfigurationObservation): boolean {
  return (
    observation.kind === "observed" &&
    isDeepStrictEqual(observation.before.profile, observation.after.profile) &&
    isDeepStrictEqual(observation.before.config, observation.after.config)
  );
}

export function configurationGuardPassed(observation: ConfigurationObservation): boolean {
  return (
    observation.kind === "observed" &&
    isDeepStrictEqual(observation.before.profile, observation.after.profile) &&
    observation.config_changes.every(allowedSectionChange)
  );
}

function readFingerprint(path: string): {
  readonly fingerprint: FileFingerprint;
  readonly contents: Buffer;
} {
  const resolved = realpathSync(path);
  const before = statSync(resolved, { bigint: true });
  const contents = readFileSync(resolved);
  const after = statSync(resolved, { bigint: true });
  if (before.mtimeNs !== after.mtimeNs || before.size !== after.size)
    throw new ConfigurationChanged("configuration changed during fingerprint read");
  return {
    fingerprint: {
      path: resolved,
      sha256: createHash("sha256").update(contents).digest("hex"),
      size: BigInt(contents.length),
      modified_ns: after.mtimeNs,
    },
    contents,
  };
}

export function fingerprint(path: string): FileFingerprint {
  return readFingerprint(path).fingerprint;
}

export function requireTrustedNativeCwd(config: string, cwd: string): void {
  const { contents } = readFingerprint(config);
  const parsed = objectSchema.parse(
    parse(new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(contents)),
  );
  const projects = z.record(z.string(), objectSchema).parse(parsed.projects ?? {});
  if (
    !statSync(cwd, { throwIfNoEntry: false })?.isDirectory() ||
    projects[canonicalPath(cwd)]?.trust_level !== "trusted"
  )
    throw new ConfigurationChanged(
      "native cwd has no existing exact trusted entry; choose one without writing configuration",
    );
}

export function snapshotConfiguration(
  profile: string,
  config: string,
  references: readonly string[] = [],
): CodexHomeSnapshot {
  const snapshot = readFingerprint(config);
  return {
    profile: fingerprint(profile),
    config: snapshot.fingerprint,
    config_sections: fingerprintSections(snapshot.contents, references),
  };
}

export function configurationErrorCategory(error: unknown): string {
  if (error instanceof ConfigurationChanged) return "ConfigurationChanged";
  if (error instanceof z.ZodError) return "ValidationError";
  const code =
    error !== null && typeof error === "object" && "code" in error ? error.code : undefined;
  if (code === "ENOENT") return "FileNotFoundError";
  if (code === "EACCES" || code === "EPERM") return "PermissionError";
  if (code === "EISDIR") return "IsADirectoryError";
  if (code === "ERR_ENCODING_INVALID_ENCODED_DATA") return "UnicodeDecodeError";
  if (error instanceof Error && error.name === "TomlError") return "TOMLDecodeError";
  return "RuntimeError";
}

export function observeConfiguration(input: {
  readonly before: CodexHomeSnapshot;
  readonly profile: string;
  readonly config: string;
  readonly references?: readonly string[];
}): ConfigurationObservation {
  let after: CodexHomeSnapshot;
  try {
    after = snapshotConfiguration(input.profile, input.config, input.references);
  } catch (error) {
    return { kind: "unreadable", before: input.before, error: configurationErrorCategory(error) };
  }
  return {
    kind: "observed",
    before: input.before,
    after,
    config_changes: compareSections(input.before.config_sections, after.config_sections),
    writer_attribution: "unattributed",
  };
}

export function requireReadOnlyConfiguration(observation: ConfigurationObservation): void {
  if (!configurationGuardPassed(observation))
    throw new ConfigurationChanged(
      "saved profile changed, configuration attribution failed or became unreadable; left untouched",
    );
}

export function registerGuardIdentifiers(evidence: string, value: unknown): void {
  const identifiers = new Set<string>();
  function collect(item: unknown): void {
    if (Array.isArray(item)) {
      for (const child of item) collect(child);
      return;
    }
    const parsed = objectSchema.safeParse(item);
    if (!parsed.success) return;
    for (const [key, child] of Object.entries(parsed.data)) {
      if (
        [
          "sessionId",
          "requestId",
          "pushId",
          "turnId",
          "inputId",
          "operationId",
          "serviceId",
          "serviceEpoch",
        ].includes(key) &&
        typeof child === "string"
      )
        identifiers.add(child);
      collect(child);
    }
  }
  collect(value);
  const path = join(evidence, "guard-identifiers.jsonl");
  appendFileSync(path, `${evidenceJson([...identifiers].sort())}\n`);
  chmodSync(path, 0o600);
}

export function configurationReferences(evidence: string, paths: readonly string[]): string[] {
  const references = new Set(paths.map((path) => canonicalPath(path)));
  const ledger = join(evidence, "guard-identifiers.jsonl");
  if (existsSync(ledger)) {
    const lines = readFileSync(ledger, "utf8").split("\n");
    if (lines.at(-1) === "") lines.pop();
    for (const line of lines) {
      const values = z.array(z.string()).parse(decode(line));
      if (values.some((value) => value.length === 0))
        throw new Error("invalid run identity ledger");
      for (const value of values) references.add(value);
    }
  }
  return [...references].sort();
}
