import assert from "node:assert/strict";
import { test } from "node:test";
import { parse } from "lossless-json";
import { buildCheckpoint, exactIntegerText } from "./prepare-matrix.ts";

test("Python-port oracle keeps exact integer, blob, NULL, escaped identifiers and absence checks", () => {
  const input: unknown = parse(
    '{"name":"exact","typedRows":{"a\\\"b":[[{"kind":"integer","value":9007199254740993},{"kind":"blob","value":"AP+A"},{"kind":"null"}]]},"columns":{"a\\\"b":["id","bytes","missing"]},"absentTables":["owner\u0027s"],"schemaIdentity":{"version":1,"fingerprint":"literal"},"position":{"epoch":2,"sequence":3}}',
  );
  const result = buildCheckpoint(input);
  assert.deepEqual(result.expected[0], [
    ["integer", "9007199254740993", "blob", "00FF80", "null", null],
  ]);
  assert.ok(result.queries[0]?.includes('FROM "a""b" ORDER BY "id"'));
  assert.equal(
    result.queries[2],
    "SELECT json_array(name,sql) AS result FROM sqlite_schema WHERE name='owner''s' OR instr(coalesce(sql,''),'owner''s')>0",
  );
  assert.equal(result.expected.length, 4);
  assert.equal(exactIntegerText(4), "4");
});

test("manifest port retains final-part matching, offline streams and optional snapshots", async () => {
  const { mkdtempDisposableSync, mkdirSync, writeFileSync, readFileSync } = await import("node:fs");
  const { join } = await import("node:path");
  const { tmpdir } = await import("node:os");
  const { z } = await import("zod");
  const { prepareMatrix } = await import("./prepare-matrix.ts");
  const temporary = mkdtempDisposableSync(join(tmpdir(), "fleet-matrix-port-"));
  try {
    const fixtures = join(temporary.path, "fixtures");
    const fixture = join(fixtures, "literal");
    const offline = join(fixture, "offline-stream");
    mkdirSync(offline, { recursive: true });
    const checkpoint =
      '{"name":"final","typedRows":{"samples":[[{"kind":"integer","value":9007199254740993}]]},"columns":{"samples":["id"]},"schemaIdentity":{"version":1,"fingerprint":"literal"},"position":{"epoch":2,"sequence":9007199254740993}}';
    const through = '"through":{"epoch":2,"sequence":9007199254740993}';
    for (const directory of [fixture, offline]) {
      writeFileSync(
        join(directory, "create-copy.json"),
        '{"database":"019eef00-0000-7000-8000-000000000001"}',
      );
      writeFileSync(join(directory, "sender.json"), '"019eef00-0000-7000-8000-000000000002"');
      writeFileSync(join(directory, "checkpoint-1.json"), checkpoint);
    }
    writeFileSync(join(fixture, "case-status.json"), '{"status":"passed"}');
    writeFileSync(join(fixture, "piece-1.json"), "{" + through + ',"part":{"index":0,"count":2}}');
    writeFileSync(join(fixture, "piece-2.json"), "{" + through + ',"part":{"index":1,"count":2}}');
    writeFileSync(join(offline, "piece-1.json"), "{" + through + "}");
    writeFileSync(
      join(fixture, "snapshot-manifest.json"),
      '{"database":"019eef00-0000-7000-8000-000000000001","position":{"epoch":2,"sequence":9007199254740993},"schema":{"version":1}}',
    );
    writeFileSync(join(fixture, "source-copy-status.json"), "{}");
    writeFileSync(join(fixture, "snapshot-page-1.json"), '{"next":{"after":"first"}}');
    writeFileSync(join(fixture, "snapshot-page-2.json"), '{"next":null}');
    const destination = join(temporary.path, "manifest.json");
    await prepareMatrix(destination, fixtures);
    const raw = readFileSync(destination, "utf8");
    const result = z
      .object({
        cases: z.array(
          z.object({
            name: z.string(),
            checkpoints: z.record(z.string(), z.unknown()),
            targetProduced: z.boolean(),
          }),
        ),
        snapshots: z.array(
          z.object({ name: z.string(), pages: z.array(z.object({ cursor: z.unknown() })) }),
        ),
      })
      .parse(JSON.parse(raw));
    assert.deepEqual(
      result.cases.map((row) => [row.name, Object.keys(row.checkpoints), row.targetProduced]),
      [
        ["literal", ["2"], true],
        ["literal-offline", ["1"], true],
      ],
    );
    assert.equal(result.snapshots[0]?.name, "literal-new-snapshot");
    assert.deepEqual(
      result.snapshots[0]?.pages.map((page) => page.cursor),
      [null, { after: "first" }],
    );
    assert.ok(raw.includes('"sequence": 9007199254740993'));
    assert.ok(raw.includes('"9007199254740993"'));
  } finally {
    temporary.remove();
  }
});
