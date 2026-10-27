import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

export interface ExecObservationInput {
  readonly token: string;
  readonly arguments: readonly string[];
  readonly stdin: string;
}
export function observeExec(input: ExecObservationInput, command: readonly string[]): unknown {
  const root = mkdtempSync(join(tmpdir(), "2026-10-token-exec-"));
  try {
    const [executable, ...arguments_] = command;
    assert.ok(executable !== undefined);
    const probe =
      "process.stdout.write(JSON.stringify({pid:process.pid,args:process.argv.slice(1),token:process.env.ANTHROPIC_AUTH_TOKEN})+'\\n');process.stdin.pipe(process.stdout)";
    const output = spawnSync(
      executable,
      [...arguments_, join(root, "token"), process.execPath, "-e", probe, "--", ...input.arguments],
      {
        env: { ...process.env, ANTHROPIC_AUTH_TOKEN: input.token },
        input: input.stdin,
        encoding: "utf8",
        timeout: 5000,
      },
    );
    if (output.error !== undefined) throw output.error;
    const newline = output.stdout.indexOf("\n");
    const receipt: unknown = JSON.parse(output.stdout.slice(0, newline));
    assert.ok(receipt !== null && typeof receipt === "object" && "pid" in receipt);
    assert.equal(receipt.pid, output.pid, "exec must retain the wrapper PID");
    return {
      exit: output.status,
      stderr: output.stderr,
      stdout: output.stdout.slice(newline + 1),
      tokenFile: readFileSync(join(root, "token"), "utf8"),
      receipt: { ...receipt, pid: "<OWN_PID>" },
    };
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}
