import assert from "node:assert/strict";
import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  realpathSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { test } from "node:test";
import { z } from "zod";
import { prepareSqlxMetadata, sqlitePreparationTargets } from "../prepare-sqlx.ts";
import { prepareTursoMetadata, tursoPreparationTargets } from "../prepare-sqlx-turso.ts";
import { describeMetadataDifferences, readQueryMetadata } from "../query-metadata-files.ts";
import type { CommandRunner } from "../process-command.ts";

const callSchema = z.object({
  command: z.array(z.string()),
  environment: z.record(z.string(), z.string()),
});
const baselineSchema = z.object({
  kind: z.enum(["sqlite", "turso"]),
  mode: z.string(),
  expected: z.object({
    status: z.number(),
    calls: z.array(callSchema),
    metadata: z.record(z.string(), z.record(z.string(), z.string())),
    stderr: z.string(),
  }),
});

function normalizeText(text: string, root: string): string {
  return text
    .replaceAll(root, "<root>")
    .replace(/sqlx-(?:turso-)?schema-[^/ ]+/g, "<schema>")
    .replace(/[^ ]*\/sqlx-package-rustc-wrapper\.ts/g, "<wrapper>");
}

const fixtureDirectory = new URL("fixtures/sqlx-preparation/", import.meta.url);
for (const filename of readdirSync(fixtureDirectory).sort()) {
  const entry = baselineSchema.parse(
    JSON.parse(readFileSync(new URL(filename, fixtureDirectory), "utf8")),
  );
  test(`SQLx ${entry.kind} ${entry.mode} matches original commands, environments, exits and cache bytes`, (context): void => {
    const root = realpathSync(mkdtempSync(join(tmpdir(), "sqlx-port-")));
    const calls: z.infer<typeof callSchema>[] = [];
    const errors: string[] = [];
    context.mock.method(console, "error", (message: string): void => {
      errors.push(message);
    });
    try {
      const targets = entry.kind === "sqlite" ? sqlitePreparationTargets : tursoPreparationTargets;
      for (const target of targets) {
        const metadata =
          entry.kind === "sqlite"
            ? join(root, ".sqlx")
            : join(root, "crates", target.packageName, ".sqlx");
        mkdirSync(metadata, { recursive: true });
        const name =
          entry.mode === "check-pass" ? `query-${target.packageName}.json` : "query-original.json";
        writeFileSync(
          join(metadata, name),
          entry.mode === "check-pass" ? target.packageName : "original",
        );
      }
      const runner: CommandRunner = (command, options) => {
        const environment = Object.fromEntries(
          Object.entries(options.env ?? {})
            .filter(
              (entry): entry is [string, string] =>
                entry[1] !== undefined &&
                (entry[0].startsWith("SQLX_") ||
                  ["DATABASE_URL", "RUSTC_WRAPPER"].includes(entry[0])),
            )
            .map(([key, value]) => [key, normalizeText(value, root)]),
        );
        calls.push({ command: command.map((value) => normalizeText(value, root)), environment });
        if (command[0] === "cargo" && command[1] === "check") {
          const packageName = options.env?.SQLX_PREPARE_PACKAGE;
          const staging = options.env?.SQLX_PREPARE_METADATA_DIR;
          assert.notEqual(packageName, undefined);
          assert.notEqual(staging, undefined);
          if (packageName === undefined || staging === undefined)
            throw new Error("describing compiler has no package or metadata directory");
          if (entry.mode !== "empty")
            writeFileSync(
              join(
                staging,
                entry.mode === "conflict" ? "query-shared.json" : `query-${packageName}.json`,
              ),
              packageName,
            );
          if (
            (entry.mode === "fail-first" && packageName === targets[0]?.packageName) ||
            (entry.mode === "fail-last" && packageName === targets.at(-1)?.packageName)
          )
            return { status: 7, stdout: "", stderr: "" };
        }
        return { status: 0, stdout: "", stderr: "" };
      };
      const options = {
        repositoryRoot: root,
        checkMetadata: entry.mode.startsWith("check"),
        runProcess: runner,
      };
      const status =
        entry.kind === "sqlite" ? prepareSqlxMetadata(options) : prepareTursoMetadata(options);
      const metadata: Record<string, Record<string, string>> = {};
      for (const target of targets) {
        const relative = entry.kind === "sqlite" ? ".sqlx" : `crates/${target.packageName}/.sqlx`;
        metadata[relative] = Object.fromEntries(
          [...readQueryMetadata(join(root, relative))].map(([name, content]) => [
            name,
            content.toString("utf8"),
          ]),
        );
      }
      assert.deepEqual(
        { status, calls, metadata, stderr: errors.length === 0 ? "" : `${errors.join("\n")}\n` },
        entry.expected,
      );
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });
}

test("SQLx differences report missing, stale and changed catalog bytes in order", (): void => {
  assert.deepEqual(
    describeMetadataDifferences(
      new Map([
        ["query-old.json", Buffer.from("old")],
        ["query-changed.json", Buffer.from("before")],
      ]),
      new Map([
        ["query-new.json", Buffer.from("new")],
        ["query-changed.json", Buffer.from("after")],
      ]),
    ),
    ["missing query-new.json", "stale query-old.json", "changed query-changed.json"],
  );
});

test("real compiler wrapper process scopes online metadata and preserves compiler exit status", (): void => {
  for (const kind of ["package", "sqlite"]) {
    const prefix = kind === "package" ? "SQLX" : "SQLITE";
    for (const packageName of ["owner", "dependency"]) {
      const result = spawnSync(
        process.execPath,
        [
          new URL(`../sqlx-${kind}-rustc-wrapper.ts`, import.meta.url).pathname,
          process.execPath,
          "-e",
          "console.log(JSON.stringify([process.env.SQLX_OFFLINE,process.env.DATABASE_URL ?? null,process.env.SQLX_OFFLINE_DIR ?? null])); process.exitCode = 7;",
        ],
        {
          env: {
            ...process.env,
            [`${prefix}_PREPARE_PACKAGE`]: "owner",
            [`${prefix}_PREPARE_MANIFEST_DIR`]: "/owner",
            [`${prefix}_PREPARE_DATABASE_URL`]: "sqlite:///schema",
            SQLX_PREPARE_METADATA_DIR: "/generated",
            CARGO_PKG_NAME: packageName,
            CARGO_MANIFEST_DIR: "/owner",
            DATABASE_URL: "stale",
            SQLX_OFFLINE_DIR: "/inherited",
          },
          encoding: "utf8",
        },
      );
      assert.equal(result.status, 7, result.stderr);
      assert.deepEqual(
        JSON.parse(result.stdout),
        packageName === "dependency"
          ? ["true", null, null]
          : ["false", "sqlite:///schema", kind === "package" ? "/generated" : "/inherited"],
      );
    }
  }
});
