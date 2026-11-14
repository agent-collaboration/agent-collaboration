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
      if (child.exitCode === null && child.signalCode === null) child.kill("SIGKILL");
      for (const match of diagnostics.matchAll(/WORKERD_OWNED_PID (\d+)/g)) {
        stopRecordedWorkerd(Number(match[1]));
      }
      child.stdin.destroy();
      child.stdout.destroy();
      child.stderr.destroy();
    }
  }
});
