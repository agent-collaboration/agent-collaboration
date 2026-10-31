import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { mkdtemp, readFile, readdir, rmdir, stat, unlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, isAbsolute, join } from "node:path";
import { createInterface } from "node:readline";
import { test } from "node:test";
import * as z from "zod/mini";

const objectSchema = z.record(z.string(), z.unknown());
const envelopeSchema = z.object({
  calls: z.array(z.object({ method: z.string(), params: objectSchema, sameReceiver: z.boolean() })),
  results: z.array(z.looseObject({ method: z.string(), error: z.optional(z.string()) })),
  priorFilesAvailable: z.array(z.boolean()),
  sdkLaunches: z.array(z.object({ argv: z.array(z.string()), debugText: z.string() })),
  inputUnchanged: z.boolean(),
});

type EntryEnvelope = z.infer<typeof envelopeSchema>;
async function verifyPrivateSettings(
  call: EntryEnvelope["calls"][number],
  launched: EntryEnvelope["sdkLaunches"][number] | undefined,
  root: string,
): Promise<string> {
  const metadata = objectSchema.parse(call.params._meta);
  const options = objectSchema.parse(objectSchema.parse(metadata.claudeCode).options);
  assert.equal(
    typeof options.settings,
    "string",
    "SDK settings must be a path, never inline credential JSON",
  );
  const path = z.string().parse(options.settings);
  assert.equal(isAbsolute(path), true);
  assert.equal(dirname(dirname(path)), root);
  assert.equal((await stat(dirname(path))).mode & 0o777, 0o700);
  assert.equal((await stat(path)).mode & 0o777, 0o600);
  const expectedEnv = {
    ANTHROPIC_BASE_URL: "http://127.0.0.1:31415/anthropic",
    ANTHROPIC_AUTH_TOKEN: "literal-host-local-token",
    CLAUDE_CODE_USE_BEDROCK: "0",
    CLAUDE_CODE_USE_VERTEX: "0",
    ANTHROPIC_BEDROCK_BASE_URL: "",
    ANTHROPIC_VERTEX_BASE_URL: "",
    ANTHROPIC_VERTEX_PROJECT_ID: "",
    CLOUD_ML_REGION: "",
    AWS_REGION: "",
    ANTHROPIC_API_KEY: "",
    CLAUDE_CODE_OAUTH_TOKEN: "",
    ANTHROPIC_CUSTOM_HEADERS: "",
  };
  assert.deepEqual(JSON.parse(await readFile(path, "utf8")), {
    env: expectedEnv,
    apiKeyHelper: "",
  });
  assert.deepEqual(options.env, expectedEnv);
  // Qualified SDK0.3.280 passes a string unchanged, but JSON-stringifies an object.

  assert.ok(launched);
  assert.deepEqual(launched.argv, ["--settings", path]);
  const debugText = launched.debugText;
  assert.equal(debugText.includes("literal-host-local-token"), false);
  assert.equal(debugText.includes(path), true);
  assert.equal(call.sameReceiver, true);

  return path;
}

for (const ending of ["eof", "SIGINT", "SIGTERM", "rejected-delegate"]) {
  await test(`shipped lifecycle keeps complete private files for process lifetime on ${ending}`, async (): Promise<void> => {
    const root = await mkdtemp(join(tmpdir(), "claude-file-entry-"));
    const source = await readFile(
      new URL("../../../crates/acp-provider-adapters/src/claude_metadata.mjs", import.meta.url),
      "utf8",
    );
    const child = spawn(
      process.execPath,
      [
        "--input-type=module",
        "--eval",
        source,
        "--",
        new URL("./fixtures/public-route-library.ts", import.meta.url).href,
        new URL("./fixtures/public-sdk.ts", import.meta.url).href,
      ],
      {
        env: {
          ...process.env,
          TMPDIR: root,
          ANTHROPIC_BASE_URL: "http://127.0.0.1:31415/anthropic",
          ANTHROPIC_AUTH_TOKEN: "literal-host-local-token",
          ROUTER_ROUTE_FIXTURE_REJECT: ending === "rejected-delegate" ? "loadSession" : "",
          ROUTER_ROUTE_FIXTURE_SPAWN: "1",
          ROUTER_ROUTE_FIXTURE_CASE: JSON.stringify({
            params: { sessionId: "fixture" },
            methods: ["newSession", "loadSession", "resumeSession"],
          }),
        },
        stdio: ["pipe", "pipe", "pipe"],
        signal: AbortSignal.timeout(5000),
      },
    );
    const exit = once(child, "exit");
    try {
      let row: z.infer<typeof envelopeSchema> | undefined;
      for await (const line of createInterface({ input: child.stdout })) {
        row = envelopeSchema.parse(JSON.parse(line));
        break;
      }
      assert.ok(row);
      assert.equal(row.calls.length, 3);
      const paths: string[] = [];
      assert.deepEqual(row.priorFilesAvailable, [true, true, true]);
      if (ending === "rejected-delegate")
        assert.equal(row.results[1]?.error, "Controlled delegate rejection");
      else
        assert.equal(
          row.results.every((result) => result.error === undefined),
          true,
        );
      for (const [index, call] of row.calls.entries()) {
        const path = await verifyPrivateSettings(call, row.sdkLaunches[index], root);
        paths.push(path);
      }
      assert.equal(new Set(paths).size, 3);
      assert.equal(row.inputUnchanged, true);
      if (ending === "SIGINT" || ending === "SIGTERM") child.kill(ending);
      else child.stdin.end();
      const [code, signal] = await exit;
      assert.equal(code, 0);
      assert.equal(signal, null);
      // Process-lifetime retention is intentional; test owner removes exact files after exit.
      for (const path of paths) {
        assert.equal((await stat(path)).isFile(), true);
        await unlink(path);
      }
      for (const directory of new Set(paths.map(dirname))) await rmdir(directory);
    } finally {
      if (child.exitCode === null && child.signalCode === null) {
        child.kill("SIGTERM");
        await exit;
      }
      // Only this test-created root and its exact regular settings files are eligible.
      for (const directory of await readdir(root)) {
        const owned = join(root, directory);
        for (const file of await readdir(owned)) await unlink(join(owned, file));
        await rmdir(owned);
      }
      await rmdir(root);
    }
  });
}

await test("shipped entry filesystem creation failure invokes no lifecycle delegate and emits bounded errors", async (): Promise<void> => {
  const root = await mkdtemp(join(tmpdir(), "claude-file-failure-"));
  const blockedRoot = join(root, "not-a-directory");
  await writeFile(blockedRoot, "unrelated-sentinel");
  const source = await readFile(
    new URL("../../../crates/acp-provider-adapters/src/claude_metadata.mjs", import.meta.url),
    "utf8",
  );
  const child = spawn(
    process.execPath,
    [
      "--input-type=module",
      "--eval",
      source,
      "--",
      new URL("./fixtures/public-route-library.ts", import.meta.url).href,
      new URL("./fixtures/public-sdk.ts", import.meta.url).href,
    ],
    {
      env: {
        ...process.env,
        TMPDIR: blockedRoot,
        ANTHROPIC_BASE_URL: "http://127.0.0.1:31415/anthropic",
        ANTHROPIC_AUTH_TOKEN: "literal-host-local-token",
        ROUTER_ROUTE_FIXTURE_CASE: JSON.stringify({
          params: { sessionId: "fixture" },
          methods: ["newSession", "loadSession", "resumeSession"],
        }),
      },
      stdio: ["pipe", "pipe", "pipe"],
      signal: AbortSignal.timeout(5000),
    },
  );
  const exit = once(child, "exit");
  let stderr = "";
  child.stderr.setEncoding("utf8").on("data", (value: string): void => {
    stderr += value;
  });
  try {
    let row: z.infer<typeof envelopeSchema> | undefined;
    for await (const line of createInterface({ input: child.stdout })) {
      row = envelopeSchema.parse(JSON.parse(line));
      break;
    }
    assert.ok(row, stderr);
    assert.deepEqual(row.calls, []);
    assert.equal(row.results.length, 3);
    for (const result of row.results)
      assert.equal(result.error, "Qualified Claude settings file failed: Creation");
    assert.equal(stderr.includes("literal-host-local-token"), false);
    assert.equal(row.inputUnchanged, true);
    assert.deepEqual(await readdir(root), ["not-a-directory"]);
    assert.equal(await readFile(blockedRoot, "utf8"), "unrelated-sentinel");
    child.stdin.end();
    const [code, signal] = await exit;
    assert.equal(code, 0);
    assert.equal(signal, null);
  } finally {
    if (child.exitCode === null && child.signalCode === null) {
      child.kill("SIGTERM");
      await exit;
    }
    await unlink(blockedRoot);
    await rmdir(root);
  }
});
