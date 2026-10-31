import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { mkdtemp, readFile, readdir, rmdir, unlink } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createInterface } from "node:readline";
import { test } from "node:test";
import * as z from "zod/mini";
import { modelReasoningSchema } from "../src/model-metadata.ts";
import { captureHostRoute, prepareClaudeRoute } from "../src/route-preparation.ts";

const lifecycleMethods = ["newSession", "loadSession", "resumeSession"];
const hostRoute = {
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
const recordSchema = z.record(z.string(), z.unknown());
const envelopeSchema = z.object({
  calls: z.array(z.object({ method: z.string(), params: recordSchema, sameReceiver: z.boolean() })),
  results: z.array(z.looseObject({ method: z.string(), error: z.optional(z.string()) })),
  forbiddenAccesses: z.array(z.string()),
  inputUnchanged: z.boolean(),
  ambientBaseUrl: z.string(),
  ambientToken: z.string(),
  ambientModelConfig: z.optional(z.string()),
  home: z.string(),
});

async function runEntry(
  params: unknown,
  methods: string[] = [...lifecycleMethods, "setSessionConfigOption"],
  modelConfig?: string,
): Promise<z.infer<typeof envelopeSchema>> {
  const temporaryRoot = await mkdtemp(join(tmpdir(), "route-entry-test-"));
  const source = await readFile(
    new URL("../../../crates/acp-provider-adapters/src/claude_metadata.mjs", import.meta.url),
    "utf8",
  );
  const environment: NodeJS.ProcessEnv = {
    ...process.env,
    HOME: "/synthetic-route-home",
    TMPDIR: temporaryRoot,
    ANTHROPIC_BASE_URL: hostRoute.ANTHROPIC_BASE_URL,
    ANTHROPIC_AUTH_TOKEN: hostRoute.ANTHROPIC_AUTH_TOKEN,
    ROUTER_ROUTE_FIXTURE_CASE: JSON.stringify({ params, methods }),
  };
  delete environment.CLAUDE_MODEL_CONFIG;
  delete environment.ROUTER_ROUTE_FIXTURE_MODEL_CONFIG;
  if (modelConfig !== undefined) {
    environment.CLAUDE_MODEL_CONFIG = '{"availableModels":["pre-policy-model"]}';
    environment.ROUTER_ROUTE_FIXTURE_MODEL_CONFIG = modelConfig;
  }
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
    { env: environment, stdio: ["pipe", "pipe", "pipe"], signal: AbortSignal.timeout(5000) },
  );
  let stderr = "";
  child.stderr.setEncoding("utf8").on("data", (text: string): void => {
    stderr += text;
  });
  const exit = once(child, "exit");
  try {
    let row: z.infer<typeof envelopeSchema> | undefined;
    for await (const line of createInterface({ input: child.stdout })) {
      row = envelopeSchema.parse(JSON.parse(line));
      break;
    }
    child.stdin.end();
    const [code, signal] = await exit;
    assert.equal(code, 0, stderr);
    assert.equal(signal, null);
    assert.ok(row, stderr);
    if (row.calls.length === 0) assert.deepEqual(await readdir(temporaryRoot), []);
    // Retain the prior content/caller-contract assertions, now read from real files.
    for (const call of row.calls) {
      if (call.method === "setSessionConfigOption") continue;
      const options = optionsFor(call.params);
      const settingsPath = z.string().parse(options.settings);
      const content: unknown = JSON.parse(await readFile(settingsPath, "utf8"));
      const metadata = recordSchema.parse(call.params._meta);
      const claude = recordSchema.parse(metadata.claudeCode);
      call.params = {
        ...call.params,
        _meta: {
          ...metadata,
          claudeCode: { ...claude, options: { ...options, settings: content } },
        },
      };
    }
    return row;
  } finally {
    if (child.exitCode === null && child.signalCode === null) {
      child.kill("SIGTERM");
      await exit;
    }
    for (const entry of await readdir(temporaryRoot)) {
      const directory = join(temporaryRoot, entry);
      for (const file of await readdir(directory)) await unlink(join(directory, file));
      await rmdir(directory);
    }
    await rmdir(temporaryRoot);
  }
}

await test("shipped entry carries the actual Host binding in both public SDK slots once per lifecycle", async (): Promise<void> => {
  const params = { sessionId: "fixture", cwd: "/synthetic-workspace", _meta: { vendor: "kept" } };
  const row = await runEntry(params);
  assert.equal(row.ambientBaseUrl, "https://policy.invalid/anthropic");
  assert.equal(row.ambientToken, "simulated-policy-token");
  assert.equal(row.home, "/synthetic-route-home");
  assert.deepEqual(
    row.calls.map((call) => call.method),
    [...lifecycleMethods, "setSessionConfigOption"],
  );
  for (const call of row.calls.slice(0, 3)) {
    assert.equal(call.sameReceiver, true);
    assert.deepEqual(call.params, {
      ...params,
      _meta: {
        vendor: "kept",
        claudeCode: { options: { env: hostRoute, settings: { env: hostRoute, apiKeyHelper: "" } } },
      },
    });
  }
  assert.deepEqual(row.calls[3]?.params, params);
  assert.equal(row.inputUnchanged, true);
  assert.deepEqual(row.forbiddenAccesses, []);
  for (const response of row.results) {
    const result = recordSchema.parse(response.result);
    const metadata = recordSchema.parse(result._meta);
    assert.equal(metadata.vendorResult, "kept");
    const routerMetadata = recordSchema.parse(metadata.codexRouter);
    assert.equal(modelReasoningSchema.parse(routerMetadata.modelReasoning).kind, "available");
  }
});

function optionsFor(params: unknown): Record<string, unknown> {
  const request = recordSchema.parse(params);
  const metadata = recordSchema.parse(request._meta);
  const claude = recordSchema.parse(metadata.claudeCode);
  return recordSchema.parse(claude.options);
}

await test("shipped route overlay resets only its closed keys and preserves deep caller data", async (): Promise<void> => {
  const conflicting = Object.fromEntries(Object.keys(hostRoute).map((key) => [key, "conflict"]));
  const params = {
    sessionId: "fixture",
    cwd: "/synthetic-workspace",
    mcpServers: [{ name: "client-owned", args: ["kept"] }],
    _meta: {
      vendor: { nested: ["unchanged"] },
      claudeCode: {
        emitRawSDKMessages: false,
        options: {
          settingSources: ["user", "project", "local"],
          additionalDirectories: ["/other-workspace"],
          env: { ...conflicting, KEEP_ENV: "sdk-unrelated" },
          settings: {
            apiKeyHelper: "old-helper",
            permissions: { allow: ["Read"] },
            env: { ...conflicting, KEEP_SETTING_ENV: "settings-unrelated" },
          },
        },
      },
    },
  };
  const row = await runEntry(params);
  for (const call of row.calls.slice(0, 3)) {
    assert.deepEqual(call.params, {
      ...params,
      _meta: {
        ...params._meta,
        claudeCode: {
          ...params._meta.claudeCode,
          options: {
            ...params._meta.claudeCode.options,
            env: { ...hostRoute, KEEP_ENV: "sdk-unrelated" },
            settings: {
              apiKeyHelper: "",
              permissions: { allow: ["Read"] },
              env: { ...hostRoute, KEEP_SETTING_ENV: "settings-unrelated" },
            },
          },
        },
      },
    });
    assert.equal(call.sameReceiver, true);
  }
  assert.deepEqual(row.calls[3]?.params, params);
  assert.equal(row.inputUnchanged, true);
  assert.deepEqual(row.forbiddenAccesses, []);
});

const configuredModels = {
  availableModels: ["sonnet", "haiku"],
  modelOverrides: { sonnet: "fixture-alias" },
  ignoredField: { mustNotBecomeSettings: true },
};
for (const settings of [
  undefined,
  null,
  {},
  { caller: "explicit", availableModels: ["caller-model"] },
]) {
  await test(`shipped model config keeps post-policy fallback and caller precedence: ${JSON.stringify(settings)}`, async (): Promise<void> => {
    const params = {
      sessionId: "fixture",
      _meta: { claudeCode: { options: settings === undefined ? {} : { settings } } },
    };
    const rawConfig = JSON.stringify(configuredModels);
    const row = await runEntry(params, lifecycleMethods, rawConfig);
    for (const call of row.calls) {
      assert.deepEqual(optionsFor(call.params).settings, {
        ...(settings ?? {
          availableModels: configuredModels.availableModels,
          modelOverrides: configuredModels.modelOverrides,
        }),
        env: hostRoute,
        apiKeyHelper: "",
      });
      assert.equal(call.sameReceiver, true);
    }
    assert.equal(row.calls.length, 3);
    assert.equal(row.ambientModelConfig, rawConfig);
    assert.equal(row.inputUnchanged, true);
    assert.deepEqual(row.forbiddenAccesses, []);
  });
}

for (const rawConfig of [
  undefined,
  "",
  "{}",
  '{"other":1}',
  '{"availableModels":null,"modelOverrides":false}',
]) {
  await test(`shipped absent model fallback adds no invented catalogue: ${JSON.stringify(rawConfig)}`, async (): Promise<void> => {
    const row = await runEntry({ sessionId: "fixture" }, lifecycleMethods, rawConfig);
    assert.equal(row.calls.length, 3);
    for (const call of row.calls)
      assert.deepEqual(optionsFor(call.params).settings, { env: hostRoute, apiKeyHelper: "" });
    assert.equal(row.ambientModelConfig, rawConfig);
    assert.equal(row.inputUnchanged, true);
  });
}

for (const rawConfig of ['{"secret-sentinel":', "null", "[]", "[{}]", "42", "true", '"text"']) {
  await test(`shipped malformed model config refuses before delegate even with explicit settings: ${rawConfig}`, async (): Promise<void> => {
    for (const settings of [undefined, { caller: "explicit" }]) {
      const row = await runEntry(
        { sessionId: "fixture", _meta: { claudeCode: { options: { settings } } } },
        lifecycleMethods,
        rawConfig,
      );
      assert.deepEqual(row.calls, []);
      assert.equal(row.results.length, 3);
      for (const result of row.results)
        assert.equal(
          result.error,
          "Qualified Claude session creation is unsupported: UnsupportedSdkOptions",
        );
      assert.equal(row.ambientModelConfig, rawConfig);
      assert.equal(row.inputUnchanged, true);
      assert.deepEqual(row.forbiddenAccesses, []);
    }
  });
}

const unsupportedMetadata: { params: unknown; reason: string }[] = [
  ...[
    "private-input-sentinel",
    [],
    { claudeCode: "private-input-sentinel" },
    { claudeCode: [] },
    { claudeCode: null },
  ].map((metadata) => ({
    params: { sessionId: "fixture", _meta: metadata },
    reason: "MalformedMetadata",
  })),
  ...[
    null,
    [],
    "private-input-sentinel",
    { env: null },
    { env: [] },
    { env: { PRIVATE: 42 } },
    { settings: "/must-not-read-settings-file" },
    { settings: [] },
    { settings: false },
    { settings: { env: [] } },
    { settings: { env: { PRIVATE: 42 } } },
    { sandbox: {} },
    { sandbox: null },
    { sandbox: false },
  ].map((options) => ({
    params: { sessionId: "fixture", _meta: { claudeCode: { options } } },
    reason: "UnsupportedSdkOptions",
  })),
];
await test("shipped malformed metadata and unsupported SDK shapes refuse without delegation or secret text", async (): Promise<void> => {
  for (const { params, reason } of unsupportedMetadata) {
    const row = await runEntry(params, lifecycleMethods);
    assert.deepEqual(row.calls, []);
    assert.equal(row.results.length, 3);
    for (const result of row.results)
      assert.equal(result.error, `Qualified Claude session creation is unsupported: ${reason}`);
    assert.equal(row.inputUnchanged, true);
    assert.deepEqual(row.forbiddenAccesses, []);
  }
});

await test("dedicated stock setter keeps literal params and does not acquire creation routing or config validation", async (): Promise<void> => {
  const params = {
    sessionId: "fixture",
    configId: "model",
    value: "haiku",
    _meta: { vendor: "kept" },
  };
  const row = await runEntry(params, ["setSessionConfigOption"], '{"invalid":');
  assert.deepEqual(row.calls, [{ method: "setSessionConfigOption", params, sameReceiver: true }]);
  assert.equal(row.inputUnchanged, true);
  assert.deepEqual(row.forbiddenAccesses, []);
});

await test("Host capture is immutable and bounded, and preparation keeps unrelated nested references", (): void => {
  const environment = {
    ANTHROPIC_BASE_URL: hostRoute.ANTHROPIC_BASE_URL,
    ANTHROPIC_AUTH_TOKEN: hostRoute.ANTHROPIC_AUTH_TOKEN,
  };
  const captured = captureHostRoute(environment);
  environment.ANTHROPIC_BASE_URL = "https://changed.invalid";
  environment.ANTHROPIC_AUTH_TOKEN = "changed-token";
  assert.equal(Object.isFrozen(captured), true);
  assert.deepEqual(captured, {
    baseUrl: hostRoute.ANTHROPIC_BASE_URL,
    localToken: hostRoute.ANTHROPIC_AUTH_TOKEN,
  });
  for (const invalid of [
    {},
    { ANTHROPIC_BASE_URL: "file:///private-sentinel", ANTHROPIC_AUTH_TOKEN: "private-token" },
    { ANTHROPIC_BASE_URL: hostRoute.ANTHROPIC_BASE_URL, ANTHROPIC_AUTH_TOKEN: "" },
  ])
    assert.throws(() => captureHostRoute(invalid), {
      message: "Qualified Claude Host route is unavailable",
    });
  const nested = Object.freeze({ keep: Object.freeze(["same-reference"]) });
  const input = Object.freeze({ _meta: Object.freeze({ vendor: nested }) });
  const result = prepareClaudeRoute({
    params: input,
    route: captured,
    modelConfig: undefined,
    writeSettings: () => "/synthetic-settings.json",
  });
  assert.equal(result.kind, "prepared");
  if (result.kind === "prepared") {
    const metadata = recordSchema.parse(recordSchema.parse(result.params)._meta);
    assert.equal(metadata.vendor, nested);
  }
});
