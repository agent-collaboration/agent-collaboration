import { createServer } from "node:http";
import { test } from "node:test";
import assert from "node:assert/strict";
import { CentralOwner } from "../../../clients/central-owner.ts";
import { decode, encode } from "../../../exact-json.ts";
import { D4ControlClient } from "./control-client.ts";

const subject = "01900000-0000-7000-8000-000000000001";
interface ControlFixture {
  readonly owner: CentralOwner;
  readonly requests: unknown[];
  close(): Promise<void>;
}
async function fixture(answers: readonly unknown[]): Promise<ControlFixture> {
  const requests: unknown[] = [];
  const failures: unknown[] = [];
  const server = createServer((request, response): void => {
    void (async (): Promise<void> => {
      const chunks: Buffer[] = [];
      for await (const chunk of request) chunks.push(Buffer.from(chunk));
      assert.equal(request.method, "POST");
      assert.equal(request.url, "/admin/v1/conformance/runtime");
      assert.equal(request.headers["cf-access-jwt-assertion"], "synthetic-owner");
      requests.push(decode(Buffer.concat(chunks).toString("utf8")));
      const answer = answers[requests.length - 1];
      if (answer === undefined) throw new Error("fixture received an unexpected control");
      response.setHeader("content-type", "application/json");
      response.end(encode(answer));
    })().catch((error: unknown): void => {
      failures.push(error);
      response.writeHead(500).end("fixture failure");
    });
  });
  await new Promise<void>((resolve, reject): void => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  const address = server.address();
  if (address === null || typeof address === "string") throw new Error("fixture bind missing");
  return {
    owner: new CentralOwner(`http://127.0.0.1:${address.port}`, {
      "Cf-Access-Jwt-Assertion": "synthetic-owner",
    }),
    requests,
    async close(): Promise<void> {
      server.closeAllConnections();
      await new Promise<void>((resolve, reject): void => {
        server.close((error): void => (error === undefined ? resolve() : reject(error)));
      });
      assert.deepEqual(failures, []);
    },
  };
}
function client(
  input: ControlFixture,
  changes: Partial<ConstructorParameters<typeof D4ControlClient>[0]> = {},
): D4ControlClient {
  return new D4ControlClient({
    owner: input.owner,
    address: { kind: "fleet" },
    shell: "tokio",
    passivity: "observed",
    clockPassivity: "observed",
    ...changes,
  });
}

test("literal draft control requests cross the real owner HTTP serializer and preserve exact integer replies", async (): Promise<void> => {
  const input = await fixture([
    { kind: "alarm", value: { stored: 1792281630000n } },
    { kind: "faultArmed", value: { faultId: subject } },
    { kind: "fault", value: { armed: 9007199254740993n, reached: 1n, consumed: 1n } },
  ]);
  try {
    const controls = client(input, { address: { kind: "copy", value: subject } });
    assert.equal(await controls.readAlarm(), 1792281630000n);
    assert.equal(
      await controls.armFault({ kind: "exactAlarmWrite", value: { on: "frame" } }, 1n),
      subject,
    );
    assert.equal((await controls.readFault(subject)).armed, 9007199254740993n);
    assert.deepEqual(input.requests, [
      { address: { kind: "copy", value: subject }, command: { kind: "readAlarm" } },
      {
        address: { kind: "copy", value: subject },
        command: {
          kind: "armFault",
          value: { fault: { kind: "exactAlarmWrite", value: { on: "frame" } }, count: 1n },
        },
      },
      {
        address: { kind: "copy", value: subject },
        command: { kind: "readFault", value: { faultId: subject } },
      },
    ]);
  } finally {
    await input.close();
  }
});

test("unqualified controls and the old clock wake path refuse before any request", async (): Promise<void> => {
  const input = await fixture([]);
  try {
    await assert.rejects(
      client(input, { passivity: "unqualified" }).readAlarm(),
      /passivity is unqualified/,
    );
    await assert.rejects(
      client(input, { clockPassivity: "unqualified" }).setTime("2026-11-08T00:00:00Z"),
      /wake side effect/,
    );
    assert.deepEqual(input.requests, []);
  } finally {
    await input.close();
  }
});

test("Tokio-only controls never leak onto workerd", async (): Promise<void> => {
  const input = await fixture([]);
  try {
    const controls = client(input, { shell: "workerd" });
    for (const call of [
      (): Promise<unknown> => controls.readPending(),
      (): Promise<unknown> => controls.readEvents(0n),
      (): Promise<unknown> => controls.rebuildAtQuiescence(true),
      (): Promise<unknown> => controls.setTime("2026-11-08T00:00:00Z"),
      (): Promise<unknown> => controls.barrier("beforeClaim", null),
    ])
      await assert.rejects(call, /Tokio-only/);
    assert.deepEqual(input.requests, []);
  } finally {
    await input.close();
  }
});

test("beforeClaim preserves the atomic claim boundary and replies use exact adjacent tags", async (): Promise<void> => {
  const input = await fixture([
    { kind: "barrierArmed", value: { barrierId: subject } },
    { kind: "barrier", value: { reached: true } },
    { kind: "done" },
  ]);
  try {
    const controls = client(input);
    await controls.barrier("beforeClaim", null);
    await controls.awaitBarrier(subject);
    await controls.releaseBarrier(subject);
    assert.deepEqual(input.requests[0], {
      address: { kind: "fleet" },
      command: { kind: "barrier", value: { point: "beforeClaim", unitKind: null } },
    });
  } finally {
    await input.close();
  }
});

test("the ring wrapper rejects truncation rather than relabeling it an empty quiet interval", async (): Promise<void> => {
  const input = await fixture([
    { kind: "events", value: { records: [], firstRetained: 4n, through: 3n } },
  ]);
  try {
    await assert.rejects(client(input).readEvents(0n), /truncated/);
  } finally {
    await input.close();
  }
});

test("wrong response tags and invalid fault data fail without implicit retries", async (): Promise<void> => {
  const input = await fixture([{ kind: "done" }]);
  try {
    await assert.rejects(client(input).readAlarm());
    await assert.rejects(client(input).armFault({ kind: "attemptsWrite" }, 0n));
    assert.equal(input.requests.length, 1);
  } finally {
    await input.close();
  }
});

test("fixture seed data has the exact nested wire shape and preserves integer notice positions", async (): Promise<void> => {
  const input = await fixture([{ kind: "done" }]);
  try {
    await client(input).seedFixture({
      kind: "noticeLog",
      value: {
        machine: subject,
        clearedThrough: 98n,
        positions: [99n, 100n],
        expiresAt: 1792281600000n,
      },
    });
    assert.deepEqual(input.requests[0], {
      address: { kind: "fleet" },
      command: {
        kind: "seed",
        value: {
          seed: {
            kind: "noticeLog",
            value: {
              machine: subject,
              clearedThrough: 98n,
              positions: [99n, 100n],
              expiresAt: 1792281600000n,
            },
          },
        },
      },
    });
  } finally {
    await input.close();
  }
});
