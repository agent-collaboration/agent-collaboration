import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { readFileSync, rmSync } from "node:fs";
import { createServer } from "node:http";
import { createServer as createTlsServer } from "node:https";
import { join } from "node:path";
import { promisify } from "node:util";
import { execFile } from "node:child_process";
import { test } from "node:test";
import { createFixtureDirectory } from "../test-support/fixture-directory.ts";
import { curlHttpReply, curlPostArguments, requireSystemCurl } from "./tailnet-transport.ts";
import { processExit } from "./runtime-plan.ts";

interface CurlCompletion {
  readonly exit: number;
  readonly stdout: Buffer;
  readonly stderr: Buffer;
}
function runFixtureCurl(arguments_: readonly string[]): Promise<CurlCompletion> {
  const environment: NodeJS.ProcessEnv = {
    ...process.env,
    ALL_PROXY: "http://127.0.0.1:1",
    HTTP_PROXY: "http://127.0.0.1:1",
    HTTPS_PROXY: "http://127.0.0.1:1",
    NO_PROXY: "",
    no_proxy: "",
  };
  for (const name of [
    "CURL_CA_BUNDLE",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "SSLKEYLOGFILE",
    "CURL_SSL_BACKEND",
  ])
    delete environment[name];
  return new Promise((resolve, reject): void => {
    const [executable, ...rest] = arguments_;
    if (executable === undefined) throw new Error("curl command missing");
    const child = spawn(executable, rest, { env: environment, stdio: ["pipe", "pipe", "pipe"] });
    const stdout: Buffer[] = [];
    const stderr: Buffer[] = [];
    const timer = setTimeout((): void => {
      child.kill("SIGKILL");
    }, 6000);
    child.stdin.on("error", (): void => {
      /* exit/status remains authoritative */
    });
    child.stdout.on("data", (chunk: Buffer): void => {
      stdout.push(chunk);
    });
    child.stderr.on("data", (chunk: Buffer): void => {
      stderr.push(chunk);
    });
    child.on("error", (error): void => {
      clearTimeout(timer);
      reject(error);
    });
    child.on("close", (): void => {
      clearTimeout(timer);
      const exit = processExit(child);
      if (exit === null) reject(new Error("curl did not settle"));
      else resolve({ exit, stdout: Buffer.concat(stdout), stderr: Buffer.concat(stderr) });
    });
    child.stdin.end('{"method": "tools/list"}');
  });
}

for (const entry of [
  { name: "normal", origin: null, status: 200, size: 0, unknown: false, expected: 200 },
  {
    name: "same tailnet Origin",
    origin: "https://node.tail.ts.net:8443",
    status: 200,
    size: 0,
    unknown: false,
    expected: 403,
  },
  {
    name: "foreign Origin",
    origin: "https://foreign.invalid",
    status: 200,
    size: 0,
    unknown: false,
    expected: 403,
  },
  {
    name: "redirect retained without follow",
    origin: null,
    status: 302,
    size: 0,
    unknown: false,
    expected: 302,
  },
  {
    name: "known oversized body",
    origin: null,
    status: 200,
    size: 4194305,
    unknown: false,
    expected: 63,
  },
  {
    name: "unknown oversized body",
    origin: null,
    status: 200,
    size: 4194305,
    unknown: true,
    expected: 63,
  },
]) {
  test(
    `real curl preserves original local transport contract: ${entry.name}`,
    { timeout: 10_000 },
    async (context): Promise<void> => {
      await requireSystemCurl();
      let requests = 0;
      const server = createServer(async (request, response): Promise<void> => {
        requests++;
        assert.equal(request.method, "POST");
        assert.equal(request.url, "/mcp");
        assert.equal(request.headers.host, "node.tail.ts.net:8443");
        const chunks: Buffer[] = [];
        for await (const chunk of request)
          chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(String(chunk)));
        assert.equal(Buffer.concat(chunks).toString(), '{"method": "tools/list"}');
        const payload =
          entry.size === 0
            ? Buffer.from('{"jsonrpc":"2.0","id":1,"result":{"tools":[]}}')
            : Buffer.alloc(entry.size, "x");
        response.on("error", (): void => {
          /* curl deliberately closes oversized responses */
        });
        response.writeHead(request.headers.origin === undefined ? entry.status : 403, {
          Location: "/mcp",
          Connection: "close",
          ...(entry.unknown
            ? { "Transfer-Encoding": "chunked" }
            : { "Content-Length": payload.length }),
        });
        response.end(payload);
      });
      context.after(async (): Promise<void> => {
        server.closeAllConnections();
        server.close();
        await once(server, "close");
      });
      server.listen(0, "127.0.0.1");
      await once(server, "listening");
      const address = server.address();
      if (address === null || typeof address === "string") throw new Error("listener absent");
      const target = { url: "https://node.tail.ts.net:8443/mcp" };
      const headers: Record<string, string> = {
        Host: "node.tail.ts.net:8443",
        "Content-Type": "application/json",
        Accept: "application/json, text/event-stream",
        "MCP-Protocol-Version": "2025-11-25",
      };
      if (entry.origin !== null) headers.Origin = entry.origin;
      const arguments_ = curlPostArguments(target, headers);
      arguments_[arguments_.indexOf("=https")] = "=http";
      arguments_[arguments_.indexOf(target.url)] = `http://127.0.0.1:${address.port}/mcp`;
      const result = await runFixtureCurl(arguments_);
      assert.equal(requests, 1);
      assert.ok(result.stdout.length <= 4194308);
      if (entry.expected === 63) {
        assert.equal(result.exit, 63);
        assert.throws(() => curlHttpReply(result), /exit 63/);
      } else {
        const reply = curlHttpReply(result);
        assert.equal(reply.status, entry.expected);
        assert.match(reply.body.toString(), /"tools":\[\]/);
      }
    },
  );
}

test(
  "real curl rejects an untrusted local TLS certificate under ordinary system trust",
  { timeout: 20_000 },
  async (context): Promise<void> => {
    await requireSystemCurl();
    const root = createFixtureDirectory("tailnet-tls");
    context.after((): void => {
      rmSync(root, { recursive: true, force: true });
    });
    const key = join(root, "key.pem");
    const certificate = join(root, "cert.pem");
    await promisify(execFile)(
      "/usr/bin/openssl",
      [
        "req",
        "-x509",
        "-newkey",
        "rsa:2048",
        "-nodes",
        "-keyout",
        key,
        "-out",
        certificate,
        "-days",
        "1",
        "-subj",
        "/CN=node.tail.ts.net",
      ],
      { timeout: 10_000 },
    );
    const server = createTlsServer({ key: readFileSync(key), cert: readFileSync(certificate) });
    server.on("tlsClientError", (): void => {
      /* the expected certificate rejection */
    });
    context.after(async (): Promise<void> => {
      server.closeAllConnections();
      server.close();
      await once(server, "close");
    });
    server.listen(0, "127.0.0.1");
    await once(server, "listening");
    const address = server.address();
    if (address === null || typeof address === "string") throw new Error("TLS listener absent");
    const target = { url: `https://node.tail.ts.net:${address.port}/mcp` };
    const arguments_ = curlPostArguments(target, { Host: `node.tail.ts.net:${address.port}` });
    arguments_.push("--resolve", `node.tail.ts.net:${address.port}:127.0.0.1`);
    const result = await runFixtureCurl(arguments_);
    assert.equal(result.exit, 60);
    assert.throws(() => curlHttpReply(result), /exit 60/);
  },
);
