import assert from "node:assert/strict";
import {
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmdirSync,
  statSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, isAbsolute, join } from "node:path";
import { test } from "node:test";
import { SettingsFiles } from "./settings-files.ts";
import { captureHostRoute, prepareClaudeRoute } from "./route-preparation.ts";

const route = captureHostRoute({
  ANTHROPIC_BASE_URL: "http://127.0.0.1:31415/anthropic",
  ANTHROPIC_AUTH_TOKEN: "synthetic-secret-token",
});

function removeTestRoot(root: string): void {
  for (const entry of readdirSync(root)) {
    const path = join(root, entry);
    if (statSync(path).isDirectory()) {
      for (const file of readdirSync(path)) unlinkSync(join(path, file));
      rmdirSync(path);
    } else unlinkSync(path);
  }
  rmdirSync(root);
}

await test("file owner lazily creates exact private immutable files and preserves unrelated sentinel", (): void => {
  const root = mkdtempSync(join(tmpdir(), "settings-owner-test-"));
  try {
    const sentinel = join(root, "unrelated.txt");
    writeFileSync(sentinel, "preserve");
    const owner = new SettingsFiles({ temporaryRoot: root });
    assert.deepEqual(readdirSync(root), ["unrelated.txt"]);
    const first = owner.writeSettings('{"first":"literal"}');
    const second = owner.writeSettings('{"second":"literal"}');
    assert.equal(isAbsolute(first), true);
    assert.notEqual(first, second);
    assert.equal(dirname(first), dirname(second));
    assert.equal(statSync(dirname(first)).mode & 0o777, 0o700);
    assert.equal(statSync(first).mode & 0o777, 0o600);
    assert.equal(statSync(second).mode & 0o777, 0o600);
    assert.equal(readFileSync(first, "utf8"), '{"first":"literal"}');
    assert.equal(readFileSync(second, "utf8"), '{"second":"literal"}');
    assert.equal(readFileSync(sentinel, "utf8"), "preserve");
  } finally {
    removeTestRoot(root);
  }
});

await test("validation and serialization reject before the injected writer is entered", (): void => {
  let writes = 0;
  const writer = (): string => {
    writes++;
    return "/never-used.json";
  };
  const cases: unknown[] = [
    { _meta: [] },
    { _meta: { claudeCode: { options: { settings: "/caller-file" } } } },
    ...[{}, null, false].map((sandbox) => ({ _meta: { claudeCode: { options: { sandbox } } } })),
    { _meta: { claudeCode: { options: { settings: { bigint: 1n } } } } },
  ];
  const circular: Record<string, unknown> = {};
  circular.self = circular;
  cases.push({ _meta: { claudeCode: { options: { settings: circular } } } });
  for (const params of cases)
    assert.equal(
      prepareClaudeRoute({ params, route, modelConfig: undefined, writeSettings: writer }).kind,
      "unsupported",
    );
  for (const raw of ["[", "null", "[]", "42"])
    assert.equal(
      prepareClaudeRoute({ params: {}, route, modelConfig: raw, writeSettings: writer }).kind,
      "unsupported",
    );
  assert.equal(writes, 0);
});

await test("real filesystem creation failure has a bounded error before delegation", (): void => {
  const root = mkdtempSync(join(tmpdir(), "settings-failure-test-"));
  try {
    const notDirectory = join(root, "not-a-directory");
    writeFileSync(notDirectory, "unrelated sentinel");
    const owner = new SettingsFiles({ temporaryRoot: notDirectory });
    let delegates = 0;
    const create = (): void => {
      const result = prepareClaudeRoute({
        params: {},
        route,
        modelConfig: undefined,
        writeSettings: (contents) => owner.writeSettings(contents),
      });
      if (result.kind === "prepared") delegates++;
    };
    assert.throws(create, { message: "Qualified Claude settings file failed: Creation" });
    assert.equal(delegates, 0);
    assert.deepEqual(readdirSync(root), ["not-a-directory"]);
    assert.equal(readFileSync(notDirectory, "utf8"), "unrelated sentinel");
  } finally {
    removeTestRoot(root);
  }
});

await test("controlled partial-write failure removes only its own partial file and keeps completed files", (): void => {
  const root = mkdtempSync(join(tmpdir(), "settings-partial-test-"));
  try {
    const sentinel = join(root, "unrelated.txt");
    writeFileSync(sentinel, "preserve");
    let shouldFail = false;
    const owner = new SettingsFiles({
      temporaryRoot: root,
      writeContents(descriptor, contents): void {
        writeFileSync(descriptor, shouldFail ? contents.slice(0, 4) : contents);
        if (shouldFail) throw new Error("synthetic-secret-shaped-underlying-error");
      },
    });
    const completed = owner.writeSettings('{"complete":true}');
    shouldFail = true;
    assert.throws(() => owner.writeSettings('{"partial":"synthetic-secret"}'), {
      message: "Qualified Claude settings file failed: Creation",
    });
    assert.deepEqual(readdirSync(dirname(completed)), [completed.split("/").at(-1)]);
    assert.equal(readFileSync(completed, "utf8"), '{"complete":true}');
    assert.equal(readFileSync(sentinel, "utf8"), "preserve");
  } finally {
    removeTestRoot(root);
  }
});
