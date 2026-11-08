/** Owner HTTP fixture only; no product process or runtime qualification. */
import { createServer } from "node:http";
import assert from "node:assert/strict";
import { CentralOwner } from "../../../clients/central-owner.ts";
import { decode, encode } from "../../../exact-json.ts";
export interface ControlFixture {
  readonly owner: CentralOwner;
  readonly requests: unknown[];
  close(): Promise<void>;
}
export async function fixture(
  answers: readonly unknown[] | ((request: unknown, index: number) => unknown | Promise<unknown>),
): Promise<ControlFixture> {
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
      const answer =
        typeof answers === "function"
          ? await answers(requests.at(-1), requests.length - 1)
          : answers[requests.length - 1];
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
