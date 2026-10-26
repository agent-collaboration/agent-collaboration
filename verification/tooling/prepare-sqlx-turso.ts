import { mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { runProcess, type CommandOptions, type CommandRunner } from "./process-command.ts";
import {
  describeMetadataDifferences,
  offlineEnvironment,
  readQueryMetadata,
  replaceQueryMetadata,
} from "./query-metadata-files.ts";
import type { PreparationOptions } from "./prepare-sqlx.ts";

export interface TursoPreparationTarget {
  readonly packageName: string;
  readonly migrationSources: readonly string[];
}

export const tursoPreparationTargets: readonly TursoPreparationTarget[] = [
  {
    packageName: "board-legacy-upgrade",
    migrationSources: ["crates/interaction-storage/migrations"],
  },
  {
    packageName: "interaction-storage",
    migrationSources: ["crates/interaction-storage/migrations"],
  },
  {
    packageName: "message-board-storage",
    migrationSources: [
      "project_migrations",
      "project_bookkeeping_migrations",
      "fleet_cache_migrations",
      "fleet_outbox_migrations",
      "cursor_migrations",
    ].map((name) => `crates/message-board-storage/${name}`),
  },
];

interface TursoCommandOptions {
  readonly repositoryRoot: string;
  readonly target: TursoPreparationTarget;
  readonly schemaDatabase: string;
  readonly stagingDirectory: string;
}

export function preparationCommands(
  options: TursoCommandOptions,
): readonly { readonly command: readonly string[]; readonly options: CommandOptions }[] {
  const { repositoryRoot, target, schemaDatabase, stagingDirectory } = options;
  const packageDirectory = resolve(repositoryRoot, "crates", target.packageName);
  const packageSpec = `path+${pathToFileURL(packageDirectory).href}`;
  const seedSpec = `path+${pathToFileURL(resolve(repositoryRoot, "crates/interaction-storage")).href}`;
  const offline = offlineEnvironment();
  const describing = {
    ...offline,
    RUSTC_WRAPPER: resolve(import.meta.dirname, "sqlx-package-rustc-wrapper.ts"),
    SQLX_PREPARE_PACKAGE: target.packageName,
    SQLX_PREPARE_MANIFEST_DIR: packageDirectory,
    SQLX_PREPARE_DATABASE_URL: `turso:${schemaDatabase}`,
    SQLX_PREPARE_METADATA_DIR: stagingDirectory,
  };
  return [
    {
      command: [
        "cargo",
        "run",
        "--locked",
        "--package",
        seedSpec,
        "--example",
        "seed_native_schema",
        "--",
        schemaDatabase,
        ...target.migrationSources,
      ],
      options: { cwd: repositoryRoot, env: offline, inherit: true },
    },
    {
      command: ["cargo", "clean", "--package", packageSpec],
      options: { cwd: repositoryRoot, env: offline, inherit: true },
    },
    {
      command: [
        "cargo",
        "check",
        "--locked",
        "--jobs",
        "1",
        "--package",
        packageSpec,
        "--all-targets",
        "--all-features",
      ],
      options: { cwd: repositoryRoot, env: describing, inherit: true },
    },
  ];
}

function runPreparationSteps(
  steps: ReturnType<typeof preparationCommands>,
  runner: CommandRunner,
): number {
  for (const step of steps) {
    const result = runner(step.command, step.options);
    if (result.status !== 0) return result.status;
  }
  return 0;
}

export function prepareTursoMetadata(
  options: PreparationOptions & { readonly targets?: readonly TursoPreparationTarget[] },
): number {
  const { repositoryRoot, checkMetadata } = options;
  const runner: CommandRunner = options.runProcess ?? runProcess;
  const temporaryRoot = join(repositoryRoot, "tmp");
  mkdirSync(temporaryRoot, { recursive: true });
  try {
    for (const target of options.targets ?? tursoPreparationTargets) {
      const metadataDirectory = join(repositoryRoot, "crates", target.packageName, ".sqlx");
      const directory = mkdtempSync(join(temporaryRoot, "sqlx-turso-schema-"));
      const staging = join(directory, "metadata");
      let prepared;
      try {
        mkdirSync(staging);
        const status = runPreparationSteps(
          preparationCommands({
            repositoryRoot,
            target,
            schemaDatabase: join(directory, "schema.db"),
            stagingDirectory: staging,
          }),
          runner,
        );
        if (status !== 0) return status;
        prepared = readQueryMetadata(staging);
      } finally {
        rmSync(directory, { recursive: true, force: true });
      }
      if (prepared.size === 0) {
        console.error(`${target.packageName}: no checked queries were described`);
        return 1;
      }
      const committed = readQueryMetadata(metadataDirectory);
      if (checkMetadata) {
        const differences = describeMetadataDifferences(committed, prepared);
        if (differences.length > 0) {
          console.error(
            `${target.packageName}: Turso SQLx metadata is out of date; run node verification/tooling/prepare-sqlx-turso.ts`,
          );
          console.error(differences.map((difference) => `  ${difference}`).join("\n"));
          return 1;
        }
      } else replaceQueryMetadata(metadataDirectory, prepared);
    }
    return 0;
  } catch (error) {
    console.error(
      `Turso SQLx metadata preparation failed: ${error instanceof Error ? error.message : String(error)}`,
    );
    return 1;
  }
}

export function main(arguments_: readonly string[] = process.argv.slice(2)): number {
  if (arguments_.length > 1 || (arguments_.length === 1 && arguments_[0] !== "--check")) {
    console.error("usage: prepare-sqlx-turso.ts [--check]");
    return 2;
  }
  return prepareTursoMetadata({
    repositoryRoot: resolve(import.meta.dirname, "../.."),
    checkMetadata: arguments_[0] === "--check",
  });
}

if (import.meta.main) process.exitCode = main();
