import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, readFile, realpath, symlink } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { command } from "../../../machines/owned-process.ts";
import { backupDatabase, classifyProjectFile, hashFile } from "./project-backups.ts";

test("online backup preserves committed contents and refuses destination reuse", async () => {
  const directory = await realpath(await mkdtemp(join(tmpdir(), "migration-backup-test-")));
  const source = join(directory, "source.sqlite");
  const destination = join(directory, "B.sqlite");
  const created = await command("/usr/bin/sqlite3", [
    source,
    "PRAGMA journal_mode=WAL; CREATE TABLE facts(id TEXT PRIMARY KEY,value TEXT); INSERT INTO facts VALUES('fact','kept');",
  ]);
  assert.equal(created.exit, 0);
  const before = await hashFile(source);
  const backup = await backupDatabase({ source, destination });
  assert.equal(backup.integrity, "ok");
  assert.equal(backup.sha256, await hashFile(destination));
  const observed = await command("/usr/bin/sqlite3", [
    "-readonly",
    `${pathToFileURL(destination).href}?immutable=1`,
    "SELECT value FROM facts WHERE id='fact';",
  ]);
  assert.equal(observed.exit, 0);
  assert.equal(observed.stdout.trim(), "kept");
  assert.equal(await hashFile(source), before);
  const bytes = await readFile(destination);
  await assert.rejects(backupDatabase({ source, destination }), /already exists/u);
  assert.deepEqual(await readFile(destination), bytes);
});

test("missing and symlink source never become newly created databases", async () => {
  const directory = await realpath(await mkdtemp(join(tmpdir(), "migration-backup-refusal-")));
  const missing = join(directory, "missing.sqlite");
  await assert.rejects(
    backupDatabase({ source: missing, destination: join(directory, "B.sqlite") }),
  );
  await symlink(missing, join(directory, "alias.sqlite"));
  await assert.rejects(
    backupDatabase({
      source: join(directory, "alias.sqlite"),
      destination: join(directory, "A.sqlite"),
    }),
  );
});

test("classifies live, retained and rejected epoch files without treating WAL as metadata", () => {
  assert.equal(classifyProjectFile("e3.sqlite"), "database");
  assert.equal(classifyProjectFile("e2.orphaned.sqlite"), "database");
  assert.equal(classifyProjectFile("e3.sqlite-wal"), "sqlite-runtime-sidecar");
  assert.equal(classifyProjectFile("e3.sqlite-shm"), "sqlite-runtime-sidecar");
  assert.throws(() => classifyProjectFile("e03.sqlite"), /unclassified/u);
  assert.throws(() => classifyProjectFile("unknown.sqlite"), /unclassified/u);
  assert.throws(() => classifyProjectFile("install.json"), /unclassified/u);
});
