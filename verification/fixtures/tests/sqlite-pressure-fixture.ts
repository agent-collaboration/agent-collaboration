import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { once } from "node:events";
import { rmSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { createFixtureDirectory } from "../../test-support/fixture-directory.ts";
import { withinFixtureDeadline } from "../fixture-control-socket.ts";

export async function observeSqlitePressure(programs: {
  readonly hold: readonly string[];
  readonly reset: readonly string[];
}): Promise<unknown> {
  const root = createFixtureDirectory("sqlite-pressure");
  const path = join(root, "state.sqlite");
  const database = new DatabaseSync(path, { timeout: 0 });
  const command = (parts: readonly string[]): [string, string[]] => {
    const [executable, ...arguments_] = parts;
    assert.ok(executable !== undefined);
    return [executable, arguments_];
  };
  try {
    database.exec(
      "CREATE TABLE route_band_account_states(account_id TEXT, route_band TEXT); INSERT INTO route_band_account_states VALUES ('chosen','responses'),('chosen','other'),('retained','responses')",
    );
    const [reset, resetArgs] = command(programs.reset);
    const result = spawnSync(reset, [...resetArgs, path, "chosen"], { encoding: "utf8" });
    assert.equal(result.status, 0, result.stderr);
    const rows = database
      .prepare("SELECT * FROM route_band_account_states ORDER BY account_id")
      .all();
    const [hold, holdArgs] = command(programs.hold);
    const child = spawn(hold, [...holdArgs, path, "0.2"], { stdio: ["ignore", "pipe", "pipe"] });
    const closed = once(child, "close");
    let errors = "";
    child.stderr.on("data", (bytes: Buffer): void => {
      errors += bytes.toString();
    });
    try {
      const [ready] = await withinFixtureDeadline(once(child.stdout, "data"), 3000);
      assert.equal(ready.toString(), "acquired\n");
      let blocked = false;
      try {
        database.exec("BEGIN IMMEDIATE");
      } catch {
        blocked = true;
      }
      assert.equal(blocked, true);
      const [status] = await withinFixtureDeadline(closed, 3000);
      assert.equal(status, 0, errors);
      database.exec("BEGIN IMMEDIATE; COMMIT");
      return {
        reset: { exit: result.status, stdout: result.stdout, stderr: result.stderr, rows },
        hold: { ready: ready.toString(), blocked, released: true, exit: status, stderr: errors },
      };
    } finally {
      if (child.exitCode === null && child.signalCode === null) {
        child.kill("SIGKILL");
        await closed;
      }
    }
  } finally {
    database.close();
    rmSync(root, { recursive: true, force: true });
  }
}
