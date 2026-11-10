import assert from "node:assert/strict";
import { mkdirSync, readFileSync, rmSync, symlinkSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { test } from "node:test";
import { createFixtureDirectory } from "../../test-support/fixture-directory.ts";
import { prettyEvidenceJson } from "../../evidence-json.ts";
import { readObserverJournal } from "./startup-observation.ts";

const SERVICE_ID = "00000000-0000-7000-8000-000000000001";
const HOST_EPOCH = "00000000-0000-7000-8000-000000000002";
const OBSERVER_ID = "00000000-0000-7000-8000-000000000003";

function createJournal(): {
  readonly root: string;
  readonly path: string;
  readonly connection: DatabaseSync;
} {
  const root = createFixtureDirectory("startup-journal");
  mkdirSync(join(root, "agent-communication"));
  const path = join(root, "agent-communication/session-registry.sqlite");
  const connection = new DatabaseSync(path);
  // Use the product's schema, rather than a second schema tailored to the reader.
  connection.exec(
    readFileSync(
      new URL("../../../crates/lifecycle-observation/src/journal_schema.sql", import.meta.url),
      "utf8",
    ),
  );
  connection.prepare("INSERT INTO journal_metadata VALUES (1,1,?,0,0,0)").run(OBSERVER_ID);
  return { root, path, connection };
}

function appendObservation(input: {
  readonly connection: DatabaseSync;
  readonly sequence: bigint;
  readonly change: { readonly kind: string; readonly status?: string };
  readonly generation?: number | null;
  readonly subject?: "backend" | "thread";
}): void {
  const backend = input.change.kind === "backendStatus";
  const observation = {
    observedAt: "2026-11-08T20:00:00.000Z",
    source: backend ? "hostLifecycle" : "observerLifecycle",
    scope: {
      endpoint: { serviceId: SERVICE_ID, endpointId: "codex-local" },
      generation:
        input.generation === null
          ? null
          : { serviceEpoch: HOST_EPOCH, generation: input.generation ?? 1 },
      // Backend and observer facts intentionally have different observer ids.
      observerId: backend ? HOST_EPOCH : OBSERVER_ID,
    },
    subject: { kind: input.subject ?? "backend" },
    change: input.change,
  };
  const insert = input.connection.prepare("INSERT INTO lifecycle_records VALUES (?,0,?)");
  insert.setReadBigInts(true);
  insert.run(input.sequence, prettyEvidenceJson(observation));
  const metadata = input.connection.prepare(
    "UPDATE journal_metadata SET last_sequence=? WHERE singleton=1",
  );
  metadata.setReadBigInts(true);
  metadata.run(input.sequence);
}

test("observer startup receipt joins current backend and coverage generations without copying thread rows or changing the database", (): void => {
  const fixture = createJournal();
  try {
    appendObservation({
      connection: fixture.connection,
      sequence: 1n,
      change: { kind: "threadStatus" },
      subject: "thread",
    });
    appendObservation({
      connection: fixture.connection,
      sequence: 2n,
      change: { kind: "backendStatus", status: "ready" },
    });
    appendObservation({
      connection: fixture.connection,
      sequence: 9007199254740993n,
      change: { kind: "coverageRestored" },
    });
    const before = readFileSync(fixture.path);

    const receipt = readObserverJournal({
      routerRoot: fixture.root,
      serviceId: SERVICE_ID,
      serviceEpoch: HOST_EPOCH,
    });

    assert.equal(receipt.kind, "connected");
    assert.equal(receipt.records.length, 2);
    assert.equal(receipt.throughSequence, 9007199254740993n);
    assert.equal(readFileSync(fixture.path).equals(before), true);
  } finally {
    fixture.connection.close();
    rmSync(fixture.root, { recursive: true, force: true });
  }
});

test("stale generations and later coverage loss cannot establish an observer connection", (): void => {
  const fixture = createJournal();
  const input = { routerRoot: fixture.root, serviceId: SERVICE_ID, serviceEpoch: HOST_EPOCH };
  try {
    appendObservation({
      connection: fixture.connection,
      sequence: 1n,
      change: { kind: "backendStatus", status: "ready" },
      generation: 2,
    });
    appendObservation({
      connection: fixture.connection,
      sequence: 2n,
      change: { kind: "coverageRestored" },
    });
    assert.equal(readObserverJournal(input).kind, "waiting");
    appendObservation({
      connection: fixture.connection,
      sequence: 3n,
      change: { kind: "coverageRestored" },
      generation: 2,
    });
    assert.equal(readObserverJournal(input).kind, "connected");
    assert.equal(readObserverJournal({ ...input, serviceEpoch: OBSERVER_ID }).kind, "waiting");
    appendObservation({
      connection: fixture.connection,
      sequence: 4n,
      change: { kind: "coverageLost" },
      generation: 2,
    });
    assert.equal(readObserverJournal(input).kind, "waiting");
    appendObservation({
      connection: fixture.connection,
      sequence: 5n,
      change: { kind: "backendStatus", status: "unavailable" },
      generation: 2,
    });
    assert.match(readObserverJournal(input).kind, /waiting/);
  } finally {
    fixture.connection.close();
    rmSync(fixture.root, { recursive: true, force: true });
  }
});

test("a journal receipt after a recorded cursor requires newly published facts", (): void => {
  const fixture = createJournal();
  const input = {
    routerRoot: fixture.root,
    serviceId: SERVICE_ID,
    serviceEpoch: HOST_EPOCH,
    afterSequence: 2n,
  };
  try {
    appendObservation({
      connection: fixture.connection,
      sequence: 1n,
      change: { kind: "backendStatus", status: "ready" },
    });
    appendObservation({
      connection: fixture.connection,
      sequence: 2n,
      change: { kind: "coverageRestored" },
    });
    assert.equal(readObserverJournal(input).kind, "waiting");
    appendObservation({
      connection: fixture.connection,
      sequence: 3n,
      change: { kind: "backendStatus", status: "ready" },
    });
    appendObservation({
      connection: fixture.connection,
      sequence: 4n,
      change: { kind: "coverageRestored" },
    });
    assert.equal(readObserverJournal(input).kind, "connected");
  } finally {
    fixture.connection.close();
    rmSync(fixture.root, { recursive: true, force: true });
  }
});

test("a persisted ready fact with no generation is rejected explicitly", (): void => {
  const fixture = createJournal();
  try {
    appendObservation({
      connection: fixture.connection,
      sequence: 1n,
      change: { kind: "backendStatus", status: "ready" },
      generation: null,
    });
    assert.throws(
      () =>
        readObserverJournal({
          routerRoot: fixture.root,
          serviceId: SERVICE_ID,
          serviceEpoch: HOST_EPOCH,
        }),
      /lacks a generation/,
    );
  } finally {
    fixture.connection.close();
    rmSync(fixture.root, { recursive: true, force: true });
  }
});

test("a journal symlink outside the owned root is refused before opening it", (): void => {
  const fixture = createJournal();
  const root = createFixtureDirectory("startup-journal-escape");
  try {
    mkdirSync(join(root, "agent-communication"));
    symlinkSync(fixture.path, join(root, "agent-communication/session-registry.sqlite"));
    assert.throws(
      () =>
        readObserverJournal({ routerRoot: root, serviceId: SERVICE_ID, serviceEpoch: HOST_EPOCH }),
      /outside the owned Router root/,
    );
  } finally {
    fixture.connection.close();
    rmSync(root, { recursive: true, force: true });
    rmSync(fixture.root, { recursive: true, force: true });
  }
});
