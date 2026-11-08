import assert from "node:assert/strict";
import { test } from "node:test";
import { SocketWindowObservation } from "./window-observation.ts";

function identity(index: number): string {
  return `01900000-0000-7000-8000-${index.toString(16).padStart(12, "0")}`;
}
function push(index: number, send = index): unknown {
  return {
    sendId: identity(send),
    body: {
      kind: "crate",
      value: { kind: "push", value: { item: { kind: "item", value: { id: identity(index) } } } },
    },
  };
}
function answer(send: number, result: unknown = { kind: "accepted", value: {} }): unknown {
  return {
    kind: "answer",
    value: {
      frame: identity(send),
      body: { kind: "crate", value: { kind: "pushed", value: result } },
    },
  };
}
function open(observation: SocketWindowObservation, connection = "socket-a"): void {
  observation.opened(connection);
  observation.server(connection, {
    kind: "welcome",
    value: { sessionId: identity(900), databases: [] },
  });
}
function sendSingle(
  observation: SocketWindowObservation,
  index: number,
  send = index,
  connection = "socket-a",
): void {
  const item = push(index, send);
  // The single frame echoes `frame`; only batch items use `sendId`.
  if (typeof item !== "object" || item === null || !("body" in item)) throw new Error("fixture");
  observation.server(connection, {
    kind: "request",
    value: { frame: identity(send), body: item.body },
  });
}

test("33 outstanding items in two legal batches exceed the window even though there are only two frames", (): void => {
  const observation = new SocketWindowObservation();
  open(observation);
  observation.server("socket-a", {
    kind: "requestBatch",
    value: Array.from({ length: 32 }, (_, index): unknown => push(index + 1)),
  });
  observation.server("socket-a", { kind: "requestBatch", value: [push(33)] });
  assert.equal(observation.snapshot().peak, 33);
  assert.throws((): void => observation.requireWindow(), /32/);
});

test("answers free slots item by item and independent connections each have their own window", (): void => {
  const observation = new SocketWindowObservation();
  open(observation);
  open(observation, "socket-b");
  for (let index = 1; index <= 32; index += 1) sendSingle(observation, index);
  sendSingle(observation, 100, 100, "socket-b");
  observation.client("socket-a", answer(1));
  sendSingle(observation, 33);
  observation.requireWindow();
  assert.equal(observation.snapshot().peak, 32);
  assert.deepEqual(
    observation.snapshot().connections.map((row) => row.current),
    [32, 1],
  );
});

test("retryable refusal retains the slot, a retry replaces its send id, and an old answer cannot release it", (): void => {
  const observation = new SocketWindowObservation();
  open(observation);
  sendSingle(observation, 1);
  observation.client(
    "socket-a",
    answer(1, { kind: "refused", value: { reason: { kind: "mailboxFull" } } }),
  );
  assert.equal(observation.snapshot().connections[0]?.current, 1);
  sendSingle(observation, 1, 101);
  observation.client("socket-a", answer(1));
  assert.equal(observation.snapshot().connections[0]?.current, 1);
  observation.client("socket-a", answer(101));
  assert.equal(observation.snapshot().connections[0]?.current, 0);
  assert.equal(observation.snapshot().staleAnswers, 1);
  observation.requireWindow();
});

test("status versions share a slot and a superseded version's answer is stale", (): void => {
  const observation = new SocketWindowObservation();
  open(observation);
  const status = (send: number, version: bigint): unknown => ({
    kind: "request",
    value: {
      frame: identity(send),
      body: {
        kind: "crate",
        value: {
          kind: "publishStatus",
          value: { publication: { item: identity(1), version } },
        },
      },
    },
  });
  observation.bindPublicationSubject("socket-a", identity(1), identity(901));
  observation.bindPublicationSubject("socket-a", identity(2), identity(901));
  observation.server("socket-a", status(1, 9007199254740993n));
  observation.server("socket-a", status(2, 9007199254740994n));
  const published = (send: number): unknown => ({
    kind: "answer",
    value: {
      frame: identity(send),
      body: { kind: "crate", value: { kind: "published", value: { kind: "applied" } } },
    },
  });
  observation.client("socket-a", published(1));
  assert.equal(observation.snapshot().connections[0]?.current, 1);
  observation.client("socket-a", published(2));
  assert.equal(observation.snapshot().connections[0]?.current, 0);
  assert.equal(observation.snapshot().peak, 1);
});

test("two status destinations for the same item require distinct durable subjects and occupy two slots", (): void => {
  const observation = new SocketWindowObservation();
  open(observation);
  for (const send of [1, 2]) {
    observation.bindPublicationSubject("socket-a", identity(send), identity(900 + send));
    observation.server("socket-a", {
      kind: "request",
      value: {
        frame: identity(send),
        body: {
          kind: "crate",
          value: {
            kind: "publishStatus",
            value: { publication: { item: identity(50), version: 1n } },
          },
        },
      },
    });
  }
  assert.equal(observation.snapshot().connections[0]?.current, 2);
  const missing = new SocketWindowObservation();
  open(missing);
  assert.throws(
    (): void =>
      missing.server("socket-a", {
        kind: "request",
        value: {
          frame: identity(3),
          body: {
            kind: "crate",
            value: {
              kind: "publishStatus",
              value: { publication: { item: identity(50), version: 1n } },
            },
          },
        },
      }),
    /durable owed subject/,
  );
  assert.throws((): void => missing.requireWindow(), /invalid/);
});

test("notices and bounded holder replies are outside owed-request window", (): void => {
  const observation = new SocketWindowObservation();
  open(observation);
  observation.server("socket-a", { kind: "notice", value: { position: 100n } });
  observation.server("socket-a", {
    kind: "request",
    value: {
      frame: identity(1),
      body: {
        kind: "crate",
        value: { kind: "release", value: {} },
      },
    },
  });
  assert.equal(observation.snapshot().peak, 0);
  assert.throws((): void => observation.requireWindow(), /no owed requests/);
});

test("wrong result kind, unknown requests, reused send ids and malformed batches cannot qualify", (): void => {
  for (const run of [
    (observation: SocketWindowObservation): void =>
      observation.server("socket-a", { kind: "requestBatch", value: [] }),
    (observation: SocketWindowObservation): void =>
      observation.server("socket-a", {
        kind: "request",
        value: { frame: identity(1), body: { kind: "future" } },
      }),
    (observation: SocketWindowObservation): void => {
      sendSingle(observation, 1);
      sendSingle(observation, 2, 1);
    },
    (observation: SocketWindowObservation): void => {
      sendSingle(observation, 1);
      observation.client("socket-a", {
        kind: "answer",
        value: {
          frame: identity(1),
          body: { kind: "crate", value: { kind: "published", value: { kind: "applied" } } },
        },
      });
    },
  ]) {
    const observation = new SocketWindowObservation();
    open(observation);
    assert.throws((): void => run(observation));
    assert.throws((): void => observation.requireWindow(), /invalid/);
  }
});

test("a late answer on another connection never closes a current request and closed socket state stays separate", (): void => {
  const observation = new SocketWindowObservation();
  open(observation);
  sendSingle(observation, 1);
  open(observation, "socket-b");
  observation.client("socket-b", answer(1));
  assert.equal(observation.snapshot().connections[0]?.current, 1);
  observation.ended("socket-a");
  assert.equal(observation.snapshot().connections[0]?.active, false);
  assert.equal(observation.snapshot().connections[0]?.current, 0);
  sendSingle(observation, 1, 101, "socket-b");
  assert.equal(observation.snapshot().connections[1]?.current, 1);
});

test("a changed Welcome session cannot reset or relabel outstanding slots", (): void => {
  const observation = new SocketWindowObservation();
  open(observation);
  sendSingle(observation, 1);
  assert.throws(
    (): void =>
      observation.server("socket-a", {
        kind: "welcome",
        value: { sessionId: identity(901), databases: [] },
      }),
    /changed session/,
  );
  assert.equal(observation.snapshot().connections[0]?.current, 1);
  assert.throws((): void => observation.requireWindow(), /invalid/);
});
