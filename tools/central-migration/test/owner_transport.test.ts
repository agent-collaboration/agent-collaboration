// T2: the operator CLIs' transport and receipt file, over real local HTTP.
// - T2-F1: a redirect is refused, never followed, so the POST, its body and
//   the owner token never reach the Location's host.
// - T2-F2: --out is taken exclusively before the first request; an existing
//   file refuses the run with no request sent.

import assert from "node:assert/strict";
import { createServer, type IncomingMessage, type Server } from "node:http";
import { mkdtemp, readFile, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import {
  OutputExists,
  RedirectRefused,
  callIntoFile,
  ownerTransport,
} from "../scripts/owner_transport.ts";

interface Seen {
  readonly method: string | undefined;
  readonly url: string | undefined;
  readonly token: string | undefined;
  readonly body: string;
}

/** A local server that records every request and answers with `answer`. */
async function server(
  answer: (request: IncomingMessage) => {
    status: number;
    headers?: Record<string, string>;
    body?: string;
  },
): Promise<{ origin: string; seen: Seen[]; close: () => Promise<void> }> {
  const seen: Seen[] = [];
  const http: Server = createServer((request, response) => {
    let body = "";
    request.on("data", (chunk: Buffer) => (body += chunk.toString()));
    request.on("end", () => {
      const token = request.headers["cf-access-token"];
      seen.push({
        method: request.method,
        url: request.url,
        token: typeof token === "string" ? token : undefined,
        body,
      });
      const reply = answer(request);
      response.writeHead(reply.status, reply.headers);
      response.end(reply.body ?? "");
    });
  });
  await new Promise<void>((resolve) => http.listen(0, "127.0.0.1", resolve));
  const address = http.address();
  if (address === null || typeof address === "string") throw new Error("no port");
  return {
    origin: `http://127.0.0.1:${address.port}`,
    seen,
    close: () => new Promise((resolve) => http.close(() => resolve())),
  };
}

async function withDirectory<T>(run: (directory: string) => Promise<T>): Promise<T> {
  const directory = await mkdtemp(join(tmpdir(), "2026-11-owner-transport-"));
  try {
    return await run(directory);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
}

const INVENTORY = '{"kind":"inventory","stops":[],"rowsWritten":0}';

for (const status of [301, 302, 303, 307, 308]) {
  test(`a ${status} redirect is refused and nothing reaches its target`, async () => {
    // Arrange: the target stands in for production; the origin redirects to it.
    const target = await server(() => ({ status: 200, body: INVENTORY }));
    const origin = await server(() => ({
      status,
      headers: { location: `${target.origin}/admin/v1/migration/inventory` },
    }));
    try {
      await withDirectory(async (directory) => {
        const out = join(directory, "inventory.json");

        // Act
        const refused = await callIntoFile(
          out,
          "inventory",
          ownerTransport(origin.origin, "owner-token"),
          '{"macFiles":[]}',
        ).catch((error: unknown) => error);

        // Assert: one request to the origin, none to the target, and the refusal on record.
        assert.ok(refused instanceof RedirectRefused, String(refused));
        assert.equal(origin.seen.length, 1);
        assert.deepEqual(target.seen, []);
        assert.match(await readFile(out, "utf8"), /never follow a redirect/);
      });
    } finally {
      await origin.close();
      await target.close();
    }
  });
}

test("an existing --out file refuses the call before any request", async () => {
  const origin = await server(() => ({ status: 200, body: INVENTORY }));
  try {
    await withDirectory(async (directory) => {
      // Arrange: an earlier answer already sits at the path.
      const out = join(directory, "inventory.json");
      await writeFile(out, "earlier answer");

      // Act
      const refused = await callIntoFile(
        out,
        "inventory",
        ownerTransport(origin.origin, "owner-token"),
        '{"macFiles":[]}',
      ).catch((error: unknown) => error);

      // Assert: no request sent, and the earlier file untouched.
      assert.ok(refused instanceof OutputExists, String(refused));
      assert.deepEqual(origin.seen, []);
      assert.equal(await readFile(out, "utf8"), "earlier answer");
    });
  } finally {
    await origin.close();
  }
});

test("a direct answer is written once to a new private --out file, with the token sent as a header", async () => {
  const origin = await server(() => ({ status: 200, body: INVENTORY }));
  try {
    await withDirectory(async (directory) => {
      // Arrange
      const out = join(directory, "inventory.json");

      // Act
      const result = await callIntoFile(
        out,
        "inventory",
        ownerTransport(origin.origin, "owner-token"),
        '{"macFiles":[]}',
      );

      // Assert
      assert.equal(result.success, true);
      assert.deepEqual(origin.seen, [
        {
          method: "POST",
          url: "/admin/v1/migration/inventory",
          token: "owner-token",
          body: '{"macFiles":[]}',
        },
      ]);
      assert.equal((await stat(out)).mode & 0o777, 0o600);
      assert.deepEqual(JSON.parse(await readFile(out, "utf8")), JSON.parse(INVENTORY));
    });
  } finally {
    await origin.close();
  }
});
