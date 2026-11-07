// Local product probe: real compiled Rust Worker, native WebCrypto, fixed JOSE input.
import assert from "node:assert/strict";
import { readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { performance } from "node:perf_hooks";
import { Miniflare, Response, convertV4MiniflareOptions } from "miniflare";
import { z } from "zod";

const inputSchema = z.object({
  authorization: z.string(),
  proof: z.string(),
  request_id: z.string(),
  method: z.string(),
  body: z.string(),
  reject_through_ms: z.number().int(),
});
const fixtureSchema = z.object({
  policy: z.object({ issuer: z.string(), audience: z.string(), public_htu: z.string() }),
  trusted_jwks: z.string(),
  now_ms: z.number().int(),
  thumbprint: z.string(),
  intent: z.string(),
  vectors: z
    .array(
      z.object({
        name: z.string(),
        input: inputSchema,
        now_ms: z.number().int(),
        expected: z.string(),
      }),
    )
    .min(40),
});
const outcomeSchema = z.discriminatedUnion("kind", [
  z.object({
    kind: z.literal("verified"),
    request: z
      .object({
        proof_key_thumbprint: z.string(),
        intent_digest: z.string(),
        request_id: z.string(),
      })
      .passthrough(),
  }),
  z.object({ kind: z.literal("refused"), reason: z.string() }),
]);
const measurementSchema = z.object({
  outcomes: z.array(outcomeSchema),
  verifications: z.number().int(),
  elapsed_ms: z.number(),
  cpu_ms: z.number().nullable(),
});
const tailSchema = z.array(
  z
    .object({
      cpuTime: z.number().optional(),
      wallTime: z.number().optional(),
      outcome: z.string().optional(),
    })
    .passthrough(),
);
type Measurement = z.infer<typeof measurementSchema>;

const [outputPath] = process.argv.slice(2);
if (!outputPath) throw new Error("output JSON path required");
const fixtures = fixtureSchema.parse(
  JSON.parse(
    await readFile("crates/collaboration-central-core/tests/fixtures/person-dpop.json", "utf8"),
  ),
);
const tailMetrics: z.infer<typeof tailSchema> = [];
const runtime = new Miniflare(
  convertV4MiniflareOptions({
    cf: false,
    workers: [
      {
        name: "person-dpop-rust-prototype",
        compatibilityDate: "2026-08-21",
        modulesRoot: resolve("crates/collaboration-central-cloudflare/build"),
        modules: [
          {
            type: "ESModule",
            path: resolve("crates/collaboration-central-cloudflare/build/index.js"),
          },
          {
            type: "CompiledWasm",
            path: resolve("crates/collaboration-central-cloudflare/build/index_bg.wasm"),
          },
        ],
        durableObjects: {
          PROJECT_COPIES: { className: "ProjectCopyObject", useSQLite: true },
          FLEET: { className: "FleetObject", useSQLite: true },
          SCHEMA_PROBES: { className: "BootstrapObject", useSQLite: true },
        },
        tails: ["dpop-tail"],
      },
      {
        name: "dpop-tail",
        compatibilityDate: "2026-08-21",
        modules: true,
        script:
          "export default { async tail(events) { await fetch('http://local-tail-metrics/', {method:'POST', body:JSON.stringify(events)}); } }",
        outboundService: async (request: Request): Promise<Response> => {
          tailMetrics.push(...tailSchema.parse(await request.json()));
          return new Response("ok");
        },
      },
    ],
  }),
);

async function measure(
  input: z.infer<typeof inputSchema>,
  nowMs: number,
  repeat: number,
): Promise<Measurement & { client_wall_ms: number }> {
  const startMs = performance.now();
  const response = await runtime.dispatchFetch("http://prototype/__conformance/person-dpop", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      policy: fixtures.policy,
      trusted_jwks: fixtures.trusted_jwks,
      now_ms: nowMs,
      requests: [input],
      repeat,
    }),
  });
  if (response.status !== 200)
    throw new Error(`prototype HTTP ${response.status}: ${await response.text()}`);
  return {
    ...measurementSchema.parse(await response.json()),
    client_wall_ms: performance.now() - startMs,
  };
}

try {
  await runtime.ready;
  const vectorResults = [];
  for (const vector of fixtures.vectors) {
    const observed = await measure(vector.input, vector.now_ms, 1);
    const outcome = observed.outcomes.at(0);
    assert.ok(outcome, `${vector.name}: missing outcome`);
    const reason = outcome.kind === "verified" ? "verified" : outcome.reason;
    assert.equal(reason, vector.expected, vector.name);
    if (outcome.kind === "verified") {
      assert.equal(outcome.request.proof_key_thumbprint, fixtures.thumbprint, vector.name);
      assert.equal(outcome.request.intent_digest, fixtures.intent, vector.name);
    }
    vectorResults.push({
      name: vector.name,
      expected: vector.expected,
      observed: reason,
      worker_elapsed_ms: observed.elapsed_ms,
      client_wall_ms: observed.client_wall_ms,
      cpu_ms: observed.cpu_ms,
    });
  }
  const valid = fixtures.vectors.find((vector) => vector.name === "valid");
  assert.ok(valid);
  // Workload fixed before observing: one cold request above, 100 warmups,
  // 10 separate batches of 1000 complete verifications, no key cache.
  await measure(valid.input, fixtures.now_ms, 100);
  const samples = [];
  for (let index = 0; index < 10; index += 1) {
    const sample = await measure(valid.input, fixtures.now_ms, 1000);
    assert.equal(sample.outcomes.at(0)?.kind, "verified");
    samples.push({
      ...sample,
      worker_elapsed_ms_per_verification: sample.elapsed_ms / sample.verifications,
      client_wall_ms_per_verification: sample.client_wall_ms / sample.verifications,
    });
  }
  const report = {
    node: process.version,
    platform: process.platform,
    arch: process.arch,
    workload:
      "Ed25519 token + ES256 proof; fresh WebCrypto imports every verification; fixed validation time; no JWKS fetch or storage",
    vectors_passed: vectorResults.length,
    vectorResults,
    samples,
    local_tail_metrics: tailMetrics,
    cpu_note:
      "Worker performance.now is elapsed time. cpu_ms is null because no local invocation CPU counter is available. Tail metrics are retained as reported, including zeros/missing values; those do not establish billed CPU.",
  };
  await writeFile(outputPath, `${JSON.stringify(report, null, 2)}\n`);
  console.log(
    JSON.stringify({
      vectors_passed: vectorResults.length,
      samples: samples.map((sample) => sample.worker_elapsed_ms_per_verification),
      tailMetrics,
    }),
  );
} finally {
  await runtime.dispose();
}
