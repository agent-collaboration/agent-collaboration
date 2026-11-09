import assert from "node:assert/strict";
import { test } from "node:test";
import {
  inspectWorkerStartup,
  readWorkerEvents,
  requireIdleWorkerLog,
} from "./worker-startup-evidence.ts";

const pass = '2026-11-08T20:00:00Z DEBUG worker_loop: pass started worker="push hand-off"';
const sleep = '2026-11-08T20:00:01Z DEBUG worker_loop: sleeping worker="push hand-off" armed=None';

test("each declared worker needs a pass followed by a current sleep", (): void => {
  const events = readWorkerEvents(`unrelated log\n${pass}\n\u001b[32m${sleep}\u001b[0m\n`);
  assert.deepEqual(inspectWorkerStartup({ events, expectedWorkers: ["push hand-off"] }), [
    { worker: "push hand-off", passes: 1, sleeps: 1, deadline: null },
  ]);
  assert.equal(
    inspectWorkerStartup({ events, expectedWorkers: ["push hand-off", "schedules"] }),
    null,
  );
  assert.equal(
    inspectWorkerStartup({ events: readWorkerEvents(sleep), expectedWorkers: ["push hand-off"] }),
    null,
  );
  assert.equal(
    inspectWorkerStartup({
      events: readWorkerEvents(`${sleep}\n${pass}\n${sleep}`),
      expectedWorkers: ["push hand-off"],
    }),
    null,
    "sleeping before the first pass must not be hidden by a later event-driven pass",
  );
  assert.equal(
    inspectWorkerStartup({
      events: readWorkerEvents(`${pass}\n${sleep}\n${pass}`),
      expectedWorkers: ["push hand-off"],
    }),
    null,
  );
  assert.throws(() => inspectWorkerStartup({ events, expectedWorkers: [] }), /worker inventory/);
});

test("armed deadlines remain exact and unknown or incomplete events cannot pass", (): void => {
  const events = readWorkerEvents(sleep.replace("None", "Some(9007199254740993)"));
  assert.equal(events[0]?.kind === "sleeping" ? events[0].deadline : undefined, 9007199254740993n);
  assert.throws(
    () => readWorkerEvents(sleep.replace(" armed=None", "")),
    /lacks its armed deadline/,
  );
  assert.throws(() => readWorkerEvents(sleep.replace("sleeping", "new event")), /unrecognized/);
  assert.throws(
    () => readWorkerEvents(sleep.replace('worker="push hand-off"', "")),
    /unrecognized/,
  );
});

test("the idle check rejects even one additional pass and a truncated or rewritten log", (): void => {
  const before = readWorkerEvents(`${pass}\n${sleep}`);
  requireIdleWorkerLog({ before, after: readWorkerEvents(`${pass}\n${sleep}`) });
  assert.throws(
    () => requireIdleWorkerLog({ before, after: readWorkerEvents(`${pass}\n${sleep}\n${pass}`) }),
    /idle window/,
  );
  assert.throws(() => requireIdleWorkerLog({ before, after: [] }), /idle window/);
  assert.throws(
    () =>
      requireIdleWorkerLog({
        before,
        after: readWorkerEvents(`${pass.replace("20:00:00", "20:00:02")}\n${sleep}`),
      }),
    /idle window/,
  );
});
