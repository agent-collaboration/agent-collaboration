import assert from "node:assert/strict";
import { createServer, type IncomingMessage, type ServerResponse } from "node:http";
import type { AddressInfo } from "node:net";
import { test } from "node:test";
import { CentralOwner } from "../../../clients/central-owner.ts";
import { decode } from "../../../exact-json.ts";
import { readNativeSessionInventory } from "./native-session-inventory.ts";

const service = "01900000-0000-7000-8000-000000000001";
async function fixture(handler: (request: IncomingMessage, response: ServerResponse) => void) {
  const server = createServer(handler);
  await new Promise<void>((resolve, reject): void => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  return {
    origin: `http://127.0.0.1:${(server.address() as AddressInfo).port}`,
    close: (): Promise<void> =>
      new Promise((resolve, reject): void => {
        server.closeAllConnections();
        server.close((error) => (error ? reject(error) : resolve()));
      }),
  };
}
test("the native owner route crosses real HTTP without widening the general admin client", async (): Promise<void> => {
  const requests: {
    method: string | undefined;
    path: string | undefined;
    assertion: string | string[] | undefined;
    body: unknown;
  }[] = [];
  const input = await fixture((request, response): void => {
    const chunks: Buffer[] = [];
    request.on("data", (chunk: Buffer): void => {
      chunks.push(chunk);
    });
    request.on("end", (): void => {
      requests.push({
        method: request.method,
        path: request.url,
        assertion: request.headers["cf-access-jwt-assertion"],
        body: decode(Buffer.concat(chunks).toString()),
      });
      response.setHeader("content-type", "application/json");
      response.end('[{"counter":9007199254740993}]');
    });
  });
  try {
    await assert.rejects(
      new CentralOwner(input.origin, {}).response({
        path: "/__conformance/admission",
        method: "POST",
      }),
      /outside admin routes/,
    );
    assert.equal(requests.length, 0);
    assert.deepEqual(
      await readNativeSessionInventory({
        origin: input.origin,
        assertion: "synthetic-owner",
        service,
      }),
      [{ counter: 9007199254740993n }],
    );
    assert.deepEqual(requests, [
      {
        method: "POST",
        path: "/__conformance/admission",
        assertion: "synthetic-owner",
        body: { kind: "sessionConnections", value: { service } },
      },
    ]);
  } finally {
    await input.close();
  }
});
test("native inventory refuses non-isolated origins before network access", async (): Promise<void> => {
  for (const origin of [
    "http://127.0.0.1:19741",
    "http://127.0.0.1:43127",
    "https://127.0.0.1:45678",
    "http://example.invalid:45678",
    "http://user@127.0.0.1:45678",
    "http://127.0.0.1:45678/?token=synthetic",
  ])
    await assert.rejects(
      readNativeSessionInventory({ origin, assertion: "synthetic-owner", service }),
      /isolated declared loopback/,
    );
});
test("refused or redirected native inventory is never replayed automatically", async (): Promise<void> => {
  for (const status of [401, 307]) {
    let requests = 0;
    const input = await fixture((_request, response): void => {
      requests += 1;
      response.writeHead(status, { location: "/unexpected" });
      response.end();
    });
    try {
      await assert.rejects(
        readNativeSessionInventory({ origin: input.origin, assertion: "synthetic-owner", service }),
      );
      assert.equal(requests, 1);
    } finally {
      await input.close();
    }
  }
});
