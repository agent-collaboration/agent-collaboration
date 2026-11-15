import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { createInterface } from "node:readline";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

const bridge = fileURLToPath(
  new URL(
    "../../crates/collaboration-central-cloudflare/tests/support/workerd-bridge.mts",
    import.meta.url,
  ),
);

// Record only the workerd child this bridge actually spawns; assertions stay
// outside the loader, without exposing or changing Miniflare's private API.
const observeSpawn = `
import childProcess from 'node:child_process';
import { syncBuiltinESMExports } from 'node:module';
const originalSpawn = childProcess.spawn;
childProcess.spawn = (...arguments_) => {
  const child = originalSpawn(...arguments_);
  if (String(arguments_[0]).includes('workerd'))
    process.stderr.write('WORKERD_OWNED_PID ' + child.pid + '\\n');
  return child;
};
syncBuiltinESMExports();
`;

function stopRecordedWorkerd(pid: number): void {
  try {
    process.kill(pid, "SIGKILL");
  } catch (error: unknown) {
    if (!(error instanceof Error) || !("code" in error) || error.code !== "ESRCH") throw error;
  }
}

test("a readiness failure retains its cause when real disposal also rejects", async (): Promise<void> => {
  const loader = fileURLToPath(new URL("./workerd-storage.ts", import.meta.url));
  const injectFailure = `${observeSpawn}
    const { WorkerdStorage } = await import(${JSON.stringify(new URL(`file://${loader}`).href)});
    const close = WorkerdStorage.prototype.close;
    WorkerdStorage.prototype.close = async function () {
      await close.call(this);
      throw new Error('deliberate bridge dispose failure');
    };
  `;
  const child = spawn(
    process.execPath,
    ["--import", `data:text/javascript,${encodeURIComponent(injectFailure)}`, bridge],
    { stdio: "pipe" },
  );
  const exited = once(child, "close");
  let diagnostics = "";
  child.stderr.setEncoding("utf8");
  child.stderr.on("data", (chunk: string): void => {
    diagnostics += chunk;
  });
  child.stdout.destroy();
  child.stdin.on("error", (): void => {});
  try {
    const [code] = await exited;
    assert.equal(code, 1);
    assert.match(diagnostics, /Bridge command and cleanup failed/);
    assert.match(diagnostics, /EPIPE/);
    assert.match(diagnostics, /deliberate bridge dispose failure/);
    const match = diagnostics.match(/WORKERD_OWNED_PID (\d+)/);
    assert.ok(match !== null);
    assert.throws(() => process.kill(Number(match[1]), 0), { code: "ESRCH" });
  } finally {
    child.stdin.end();
    await exited;
    for (const match of diagnostics.matchAll(/WORKERD_OWNED_PID (\d+)/g))
      stopRecordedWorkerd(Number(match[1]));
    child.stdin.destroy();
    child.stderr.destroy();
  }
});

test("readiness and command pipe failures dispose the real workerd", async (): Promise<void> => {
  for (const failedPipe of ["readiness", "command"]) {
    const child = spawn(
      process.execPath,
      ["--import", `data:text/javascript,${encodeURIComponent(observeSpawn)}`, bridge],
      { stdio: "pipe" },
    );
    const exited = once(child, "close");
    child.stdin.on("error", (error: NodeJS.ErrnoException): void => {
      if (error.code !== "EPIPE") throw error;
    });
    let diagnostics = "";
    child.stderr.setEncoding("utf8");
    child.stderr.on("data", (chunk: string): void => {
      diagnostics += chunk;
    });
    try {
      if (failedPipe === "readiness") child.stdout.destroy();
      else {
        const ready = createInterface({ input: child.stdout });
        const [line] = await once(ready, "line");
        assert.match(String(line), /"origin":"http:\/\/127\.0\.0\.1:/);
        ready.close();
        assert.match(
          diagnostics,
          /WORKERD_OWNED_PID \d+/,
          "observe ownership before breaking stderr",
        );
        child.stderr.destroy();
        child.stdin.write("{}\n");
      }
      const [code] = await exited;
      assert.equal(code, 1, `${failedPipe}: broken output must fail the bridge`);
      const matches = [...diagnostics.matchAll(/WORKERD_OWNED_PID (\d+)/g)];
      assert.equal(matches.length, 1, "the negative control must start exactly one real workerd");
      const pid = Number(matches[0]?.[1]);
      assert.ok(Number.isSafeInteger(pid) && pid > 0);
      assert.throws(
        () => process.kill(pid, 0),
        { code: "ESRCH" },
        `${failedPipe}: workerd must exit`,
      );
    } finally {
      child.stdin.end();
      await exited;
      for (const match of diagnostics.matchAll(/WORKERD_OWNED_PID (\d+)/g)) {
        stopRecordedWorkerd(Number(match[1]));
      }
      child.stdin.destroy();
      child.stdout.destroy();
      child.stderr.destroy();
    }
  }
});

test("SIGTERM awaits owned disposal before bridge exit and preserves existing listeners", async (): Promise<void> => {
  await exerciseHeldSignalDisposal(false);
});

test("EOF close and SIGTERM await the same disposal before bridge and workerd exit", async (): Promise<void> => {
  await exerciseHeldSignalDisposal(true);
});

async function exerciseHeldSignalDisposal(startWithEof: boolean): Promise<void> {
  const miniflare = fileURLToPath(
    new URL("../node_modules/miniflare/dist/src/index.js", import.meta.url),
  );
  const injectDisposalGate = `${observeSpawn}
    const { Miniflare } = await import(${JSON.stringify(new URL(`file://${miniflare}`).href)});
    process.on('SIGTERM', () => process.stderr.write('PREEXISTING_SIGNAL_LISTENER\\n'));
    const { Socket } = await import('node:net');
    const releaseInput = new Socket({fd:3});
    const release = new Promise(resolve => {releaseInput.once('data', resolve); releaseInput.once('end', resolve);});
    const dispose = Miniflare.prototype.dispose;
    Miniflare.prototype.dispose = async function () {
      process.stderr.write('OWNED_DISPOSE_STARTED\\n');
      await release;
      await dispose.call(this);
      process.stderr.write('OWNED_DISPOSE_FINISHED\\n');
    };
  `;
  const child = spawn(
    process.execPath,
    ["--import", `data:text/javascript,${encodeURIComponent(injectDisposalGate)}`, bridge],
    { stdio: ["pipe", "pipe", "pipe", "pipe"] },
  );
  const exited = once(child, "close");
  const releaseInput = child.stdio[3];
  assert.ok(releaseInput !== null && releaseInput !== undefined && "write" in releaseInput);
  child.stdin.on("error", (error: NodeJS.ErrnoException): void => {
    if (error.code !== "EPIPE") throw error;
  });
  let diagnostics = "";
  child.stderr.setEncoding("utf8");
  child.stderr.on("data", (chunk: string): void => {
    diagnostics += chunk;
  });
  const errorLines = createInterface({ input: child.stderr });
  const started = (async (): Promise<void> => {
    for await (const line of errorLines) if (line === "OWNED_DISPOSE_STARTED") return;
    throw new Error("bridge ended before owned disposal started");
  })();
  try {
    const ready = createInterface({ input: child.stdout });
    const [line] = await Promise.race([
      once(ready, "line"),
      exited.then((): never => {
        throw new Error("bridge ended before readiness");
      }),
    ]);
    assert.match(String(line), /"origin":"http:\/\/127\.0\.0\.1:/);
    ready.close();
    if (startWithEof) {
      child.stdin.end();
      await Promise.race([
        started,
        exited.then((): never => {
          throw new Error("EOF close exited before disposal release");
        }),
      ]);
    }
    assert.equal(child.kill("SIGTERM"), true);
    await Promise.race([
      started,
      exited.then((): never => {
        throw new Error("bridge exited before disposal release");
      }),
    ]);
    assert.equal(child.exitCode, null, "signal must wait for the held real disposal");
    releaseInput.write("release disposal\n");
    const [code, signal] = await exited;
    assert.ok(startWithEof ? code === 143 || code === 0 : code === 143, `unexpected exit ${code}`);
    assert.equal(signal, null);
    assert.match(diagnostics, /PREEXISTING_SIGNAL_LISTENER/);
    assert.match(diagnostics, /OWNED_DISPOSE_FINISHED/);
    assert.equal(
      [...diagnostics.matchAll(/OWNED_DISPOSE_STARTED/g)].length,
      1,
      "one disposal promise must own EOF and signals",
    );
    assert.equal([...diagnostics.matchAll(/OWNED_DISPOSE_FINISHED/g)].length, 1);
    const matches = [...diagnostics.matchAll(/WORKERD_OWNED_PID (\d+)/g)];
    assert.equal(matches.length, 1);
    assert.throws(() => process.kill(Number(matches[0]?.[1]), 0), { code: "ESRCH" });
  } finally {
    releaseInput.end("release disposal\n");
    child.stdin.end();
    await exited;
    errorLines.close();
    for (const match of diagnostics.matchAll(/WORKERD_OWNED_PID (\d+)/g))
      stopRecordedWorkerd(Number(match[1]));
    child.stdout.destroy();
    child.stderr.destroy();
  }
}

test("standalone constructor guard disposes before refusing an absent upstream signal hook", async (): Promise<void> => {
  const factory = new URL("../test-support/miniflare-signal-shutdown.ts", import.meta.url).href;
  const program = `
    import assert from 'node:assert/strict';
    import { constructMiniflareWithOwnedSignalShutdown } from ${JSON.stringify(factory)};
    let disposed = false;
    await assert.rejects(constructMiniflareWithOwnedSignalShutdown(() => ({dispose:async () => {disposed = true;}})), /did not add expected signal hooks: SIGINT, SIGTERM, SIGHUP/);
    assert.equal(disposed,true);
    await assert.rejects(constructMiniflareWithOwnedSignalShutdown(() => ({dispose:async () => {throw new Error('guard cleanup failure');}})),
      error => error instanceof AggregateError && error.errors.length === 2 &&
        /did not add expected signal hooks/.test(error.errors[0].message) &&
        error.errors[1].message === 'guard cleanup failure' && error.cause === error.errors[0]);
  `;
  const child = spawn(process.execPath, ["--input-type=module", "-e", program], { stdio: "pipe" });
  const exited = once(child, "close");
  child.stdout.resume();
  let errors = "";
  child.stderr.setEncoding("utf8");
  child.stderr.on("data", (chunk: string): void => {
    errors += chunk;
  });
  child.stdin.end();
  const [code] = await exited;
  assert.equal(code, 0, errors);
});
