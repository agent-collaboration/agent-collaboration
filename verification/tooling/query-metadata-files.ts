import {
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { join } from "node:path";

export type QueryMetadata = ReadonlyMap<string, Buffer>;

export function readQueryMetadata(directory: string): QueryMetadata {
  if (!existsSync(directory)) return new Map();
  return new Map(
    readdirSync(directory)
      .filter((name) => name.startsWith("query-") && name.endsWith(".json"))
      .sort()
      .map((name) => [name, readFileSync(join(directory, name))]),
  );
}

export function replaceQueryMetadata(directory: string, metadata: QueryMetadata): void {
  mkdirSync(directory, { recursive: true });
  for (const name of readdirSync(directory))
    if (name.startsWith("query-") && name.endsWith(".json")) unlinkSync(join(directory, name));
  for (const [name, content] of [...metadata].sort(([left], [right]) => left.localeCompare(right)))
    writeFileSync(join(directory, name), content);
}

export function describeMetadataDifferences(
  committed: QueryMetadata,
  prepared: QueryMetadata,
): string[] {
  return [
    ...[...prepared.keys()]
      .filter((name) => !committed.has(name))
      .sort()
      .map((name) => `missing ${name}`),
    ...[...committed.keys()]
      .filter((name) => !prepared.has(name))
      .sort()
      .map((name) => `stale ${name}`),
    ...[...committed.keys()]
      .filter((name) => {
        const current = committed.get(name);
        const generated = prepared.get(name);
        return current !== undefined && generated !== undefined && !current.equals(generated);
      })
      .sort()
      .map((name) => `changed ${name}`),
  ];
}

export function mergeQueryMetadata(
  combined: Map<string, Buffer>,
  prepared: QueryMetadata,
  packageName: string,
): void {
  for (const [name, contents] of prepared) {
    const existing = combined.get(name);
    if (existing !== undefined && !existing.equals(contents))
      throw new Error(`${packageName} produced conflicting SQLx metadata for ${name}`);
    combined.set(name, contents);
  }
}

export function offlineEnvironment(
  environment: NodeJS.ProcessEnv = process.env,
): NodeJS.ProcessEnv {
  const offline: NodeJS.ProcessEnv = { ...environment, SQLX_OFFLINE: "true" };
  delete offline.DATABASE_URL;
  delete offline.SQLX_OFFLINE_DIR;
  return offline;
}
