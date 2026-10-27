import { existsSync, mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { runCommands, runProcess, type CommandRunner } from "./process-command.ts";
import {
  describeMetadataDifferences,
  mergeQueryMetadata,
  offlineEnvironment,
  readQueryMetadata,
  replaceQueryMetadata,
} from "./query-metadata-files.ts";

export interface SqlitePreparationTarget {
  readonly packageName: string;
  readonly migrationSources: readonly string[];
  readonly databaseFilename: string;
}

export const sqlitePreparationTargets: readonly SqlitePreparationTarget[] = [
  {
    packageName: "automation-storage",
    migrationSources: ["crates/automation-storage/migrations"],
    databaseFilename: "automation-schema.sqlite",
  },
  {
    packageName: "codex-router-state",
    migrationSources: ["crates/codex-router-state/migrations"],
    databaseFilename: "account-schema.sqlite",
  },
  {
    packageName: "collaboration-service",
    migrationSources: [
      "crates/collaboration-service/migrations",
      "crates/automation-storage/migrations",
    ],
    databaseFilename: "provider-operation-schema.sqlite",
  },
];

export interface PreparationOptions {
  readonly repositoryRoot: string;
  readonly checkMetadata: boolean;
  readonly runProcess?: CommandRunner;
}

export function prepareSqlxMetadata(options: PreparationOptions): number {
  const { repositoryRoot, checkMetadata } = options;
  const runner = options.runProcess ?? runProcess;
  const sqlx = join(repositoryRoot, "tmp/rust-tools/bin/sqlx");
  const metadataDirectory = join(repositoryRoot, ".sqlx");
  const temporaryRoot = join(repositoryRoot, "tmp");
  mkdirSync(temporaryRoot, { recursive: true });
  const original = readQueryMetadata(metadataDirectory);
  const combined = new Map<string, Buffer>();
  let completed = false;
  try {
    const directory = mkdtempSync(join(temporaryRoot, "sqlx-schema-"));
    try {
      for (const target of sqlitePreparationTargets) {
        const databaseUrl = `sqlite://${join(directory, target.databaseFilename)}`;
        const staging = join(directory, `${target.packageName}-metadata`);
        mkdirSync(staging);
        const packageDirectory = resolve(repositoryRoot, "crates", target.packageName);
        const schemaEnvironment = { ...process.env, DATABASE_URL: databaseUrl };
        const describing = {
          ...offlineEnvironment(),
          RUSTC_WRAPPER: resolve(import.meta.dirname, "sqlx-package-rustc-wrapper.ts"),
          SQLX_PREPARE_PACKAGE: target.packageName,
          SQLX_PREPARE_MANIFEST_DIR: packageDirectory,
          SQLX_PREPARE_DATABASE_URL: databaseUrl,
          SQLX_PREPARE_METADATA_DIR: staging,
        };
        const schemaCommands = [
          [sqlx, "database", "create"],
          ...target.migrationSources.map((source, index) => [
            sqlx,
            "migrate",
            "run",
            ...(index === 0 ? [] : ["--ignore-missing"]),
            "--source",
            source,
          ]),
        ];
        const schemaStatus = runCommands(
          schemaCommands,
          { cwd: repositoryRoot, env: schemaEnvironment, inherit: true },
          runner,
        );
        if (schemaStatus !== 0) return schemaStatus;
        const packageSpec = `path+${pathToFileURL(packageDirectory).href}`;
        const compileStatus = runCommands(
          [
            ["cargo", "clean", "--package", packageSpec],
            ["cargo", "check", "--locked", "--package", packageSpec, "--all-targets"],
          ],
          { cwd: repositoryRoot, env: describing, inherit: true },
          runner,
        );
        if (compileStatus !== 0) return compileStatus;
        const prepared = readQueryMetadata(staging);
        if (prepared.size === 0)
          throw new Error(`${target.packageName}: no checked queries were described`);
        mergeQueryMetadata(combined, prepared, target.packageName);
        replaceQueryMetadata(metadataDirectory, new Map([...original, ...combined]));
      }
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
    if (checkMetadata && describeMetadataDifferences(original, combined).length > 0) {
      console.error(
        "SQLite SQLx metadata is out of date; run node verification/tooling/prepare-sqlx.ts",
      );
      return 1;
    }
    replaceQueryMetadata(metadataDirectory, combined);
    completed = true;
    return 0;
  } catch (error) {
    console.error(
      `SQLx metadata preparation failed: ${error instanceof Error ? error.message : String(error)}`,
    );
    return 1;
  } finally {
    if (!completed) replaceQueryMetadata(metadataDirectory, original);
  }
}

export function main(arguments_: readonly string[] = process.argv.slice(2)): number {
  if (arguments_.length > 1 || (arguments_.length === 1 && arguments_[0] !== "--check")) {
    console.error("usage: prepare-sqlx.ts [--check]");
    return 2;
  }
  const repositoryRoot = resolve(import.meta.dirname, "../..");
  if (!existsSync(join(repositoryRoot, "tmp/rust-tools/bin/sqlx"))) {
    console.error("run scripts/tooling/bootstrap-tools.sh sqlx first");
    return 2;
  }
  const verification = runProcess(
    [join(repositoryRoot, "scripts/tooling/bootstrap-tools.sh"), "sqlx", "--check"],
    { cwd: repositoryRoot, inherit: true },
  );
  if (verification.status !== 0) return verification.status;
  return prepareSqlxMetadata({ repositoryRoot, checkMetadata: arguments_[0] === "--check" });
}

if (import.meta.main) process.exitCode = main();
