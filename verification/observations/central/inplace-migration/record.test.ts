import { test } from "node:test";
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { once } from "node:events";
import { mkdtemp, readFile, readdir, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { encode } from "../../../exact-json.ts";
import { recordSnapshot, readPrivateJson, verifyObservationSources } from "./record.ts";
import { snapshot } from "./contracts.ts";
const project = "01900000-0000-7000-8000-000000000001",
  workspace = "01900000-0000-7000-8000-000000000002",
  board = "01900000-0000-7000-8000-000000000003";

function fixtureResponse(path: string): unknown {
  if (path === "/admin/v1/copies?limit=100")
    return {
      projects: [
        {
          project,
          name: "test",
          workspace,
          board,
          boardArchive: "open",
          copySide: { kind: "notYetReported" },
          leaseSide: { kind: "notYetReported" },
        },
      ],
      nextAfter: null,
      asOf: "now",
    };
  if (path === `/admin/v1/copies/${project}/health`)
    return {
      kind: "live",
      value: {
        bookkeeping: [],
        migrationFailures: [],
        stage: { kind: "known", value: "following" },
        copy: {
          kind: "known",
          value: {
            database: project,
            role: "authoritative",
            fence: 9007199254740993n,
            holder: workspace,
            start: { epoch: 1n, sequence: 0n },
            acked: { epoch: 1n, sequence: 3n },
            schema: { version: 1n, fingerprint: "schema" },
          },
        },
        projectFingerprint: { kind: "known", value: "schema" },
        stagedParts: [],
        storageBytes: 100n,
      },
    };
  throw new Error("unexpected recorder path");
}

test("real HTTP recorder sends GET only and retains original hashed responses, never credentials", async (): Promise<void> => {
  const privateRoot = await mkdtemp(join(tmpdir(), "2026-11-migration-recorder-"));
  const requests: string[] = [],
    token = "synthetic-owner-assertion-never-recorded";
  const server = createServer((request, response): void => {
    assert.equal(request.method, "GET");
    assert.equal(request.headers["cf-access-jwt-assertion"], token);
    const path = request.url ?? "";
    requests.push(path);
    response.setHeader("content-type", "application/json");
    response.end(encode(fixtureResponse(path)));
  });
  const previous = process.env.MIGRATION_FIXTURE_ASSERTION;
  process.env.MIGRATION_FIXTURE_ASSERTION = token;
  try {
    server.listen(0, "127.0.0.1");
    await once(server, "listening");
    const address = server.address();
    assert.ok(address !== null && typeof address !== "string");
    const output = await recordSnapshot({
      centralOrigin: `http://127.0.0.1:${address.port}`,
      sourceHead: "a".repeat(40),
      routerTarget: { sourceHead: "b".repeat(40), version: "0.1.125" },
      routerProjects: [project],
      phase: "before",
      evidenceDirectory: join(privateRoot, "capture"),
      ownerAssertionEnv: "MIGRATION_FIXTURE_ASSERTION",
      routerObservationFiles: [],
    });
    const recorded = snapshot.parse(await readPrivateJson(output));
    assert.deepEqual(requests, [
      "/admin/v1/copies?limit=100",
      `/admin/v1/copies/${project}/health`,
    ]);
    const copy = recorded.copies[0];
    assert.ok(
      copy?.health.kind === "live" &&
        copy.health.value.copy.kind === "known" &&
        copy.health.value.copy.value !== null,
    );
    assert.equal(copy.health.value.copy.value.fence, 9007199254740993n);
    await verifyObservationSources(recorded);
    for (const file of await readdir(join(privateRoot, "capture")))
      assert.equal(
        (await readFile(join(privateRoot, "capture", file), "utf8")).includes(token),
        false,
      );
    const source = recorded.sources[0];
    assert.ok(source);
    source.sha256 = "0".repeat(64);
    await assert.rejects(verifyObservationSources(recorded), /fingerprint/);
    await assert.rejects(
      recordSnapshot({
        centralOrigin: `http://127.0.0.1:${address.port}`,
        sourceHead: "a".repeat(40),
        routerTarget: { sourceHead: "b".repeat(40), version: "0.1.125" },
        routerProjects: [project],
        phase: "before",
        evidenceDirectory: join(privateRoot, "capture"),
        ownerAssertionEnv: "MIGRATION_FIXTURE_ASSERTION",
        routerObservationFiles: [],
      }),
    );
  } finally {
    if (previous === undefined) delete process.env.MIGRATION_FIXTURE_ASSERTION;
    else process.env.MIGRATION_FIXTURE_ASSERTION = previous;
    await new Promise<void>((resolve, reject): void => {
      server.close((error): void => {
        if (error) reject(error);
        else resolve();
      });
    });
    await rm(privateRoot, { recursive: true });
  }
});
