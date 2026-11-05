import assert from "node:assert/strict";
import { createServer, type Server } from "node:http";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { CopyHold } from "./copy-hold.ts";
import { CentralOwner } from "./central-owner.ts";
import { Receipt } from "../receipt.ts";
import { decode, encode } from "../exact-json.ts";

const project = "019f0000-0000-7000-8000-000000008101";
const post = "019f0000-0000-7000-8000-000000008102";
const key = { destination: { kind: "copy", value: project }, call: "any" };
const otherKey = { destination: { kind: "copy", value: project }, call: "copyFence" };
const armed = { kind: "armed", value: { destination: key.destination } };

async function fixture(replies: readonly unknown[]): Promise<{
  server: Server;
  owner: CentralOwner;
  requests: unknown[];
  receipt: Receipt;
  close(): Promise<void>;
}> {
  const directory = await mkdtemp(join(tmpdir(), "copy-hold-wire-"));
  const requests: unknown[] = [];
  const server = createServer(async (request, response): Promise<void> => {
    let body = "";
    for await (const chunk of request) body += String(chunk);
    requests.push({ path: request.url, body: decode(body) });
    response.setHeader("content-type", "application/json");
    response.end(encode(replies[requests.length - 1]));
  });
  await new Promise<void>((resolve): void => {
    server.listen(0, "127.0.0.1", resolve);
  });
  const address = server.address();
  if (address === null || typeof address === "string") throw new Error("fixture listener missing");
  return {
    server,
    owner: new CentralOwner(`http://127.0.0.1:${address.port}`, {}),
    requests,
    receipt: new Receipt(directory),
    close: async (): Promise<void> => {
      await new Promise<void>((resolve, reject): void => {
        server.close((error): void => (error === undefined ? resolve() : reject(error)));
      });
      await rm(directory, { recursive: true });
    },
  };
}

function command(kind: string, handoff = false): unknown {
  return {
    path: "/admin/v1/conformance/runtime",
    body: {
      address: { kind: "fleet" },
      command: { kind, value: handoff ? { key, next: key } : { key } },
    },
  };
}

function held(cause: string): unknown {
  const piece = {
    database: project,
    epoch: 1n,
    continuesFrom: { epoch: 1n, sequence: 0n },
    through: { epoch: 1n, sequence: 1n },
    transactions: [
      {
        kind: "rows",
        rows: [{ kind: "upsert", table: "posts", key: [{ kind: "text", value: cause }] }],
      },
    ],
    digest: "literal-piece-digest",
  };
  return {
    kind: "held",
    value: {
      destination: key.destination,
      request: [...Buffer.from(encode({ request: { request: { kind: "piece", value: piece } } }))],
    },
  };
}

test("Copy hold arms the physical Fleet with exact keyed JSON and ignores another selector's state", async (): Promise<void> => {
  const target = await fixture([
    {
      dispatches: [
        { key: otherKey, state: held("unrelated") },
        { key, state: armed },
      ],
    },
  ]);
  try {
    const hold = new CopyHold({ owner: target.owner, project, receipt: target.receipt });
    const failure = await hold.arm().then(
      (): null => null,
      (error: unknown): unknown => error,
    );
    assert.deepEqual(
      { request: target.requests[0], failure },
      { request: command("holdNextDispatch"), failure: null },
    );
  } finally {
    await target.close();
  }
});

test("Copy cause handoff and release keep the same exact key and cause-post assertion", async (): Promise<void> => {
  const target = await fixture([
    { dispatches: [{ key, state: armed }] },
    { dispatches: [{ key, state: held("unrelated") }] },
    { dispatches: [{ key, state: armed }] },
    { dispatches: [{ key, state: held(post) }] },
    {
      dispatches: [
        { key: otherKey, state: armed },
        { key, state: held(post) },
      ],
    },
    { dispatches: [{ key: otherKey, state: armed }] },
  ]);
  try {
    const hold = new CopyHold({ owner: target.owner, project, receipt: target.receipt });
    await hold.arm();
    const piece = await hold.cause(post);
    await hold.close();
    assert.deepEqual(
      { requests: target.requests, through: piece.through },
      {
        requests: [
          command("holdNextDispatch"),
          command("awaitHeldDispatch"),
          command("resumeDispatchAndHoldNext", true),
          command("awaitHeldDispatch"),
          {
            path: "/admin/v1/conformance/runtime",
            body: { address: { kind: "fleet" }, command: { kind: "observe" } },
          },
          command("resumeDispatch"),
        ],
        through: { epoch: 1n, sequence: 1n },
      },
    );
  } finally {
    await target.close();
  }
});

test("Copy hold rejects an observation containing only a different dispatch key", async (): Promise<void> => {
  const target = await fixture([{ dispatches: [{ key: otherKey, state: armed }] }]);
  try {
    const hold = new CopyHold({ owner: target.owner, project, receipt: target.receipt });
    await assert.rejects(hold.arm(), /Copy hold did not arm/);
  } finally {
    await target.close();
  }
});

test("Copy close cannot treat a malformed owned key state as an absent hold", async (): Promise<void> => {
  const target = await fixture([
    { dispatches: [{ key, state: armed }] },
    { dispatches: [{ key, state: null }] },
  ]);
  try {
    const hold = new CopyHold({ owner: target.owner, project, receipt: target.receipt });
    await hold.arm();
    await assert.rejects(hold.close());
  } finally {
    await target.close();
  }
});
