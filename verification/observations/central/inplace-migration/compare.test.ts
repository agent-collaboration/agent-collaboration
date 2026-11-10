import { test } from "node:test";
import assert from "node:assert/strict";
import { randomUUIDv7 } from "node:crypto";
import { compareSnapshots, canonical } from "./compare.ts";
import { snapshot, type Snapshot, type RouterObservation } from "./contracts.ts";
import { collectCentral, type CentralRead } from "./record.ts";

function id(index: number): string {
  return `01900000-0000-7000-8000-${index.toString(16).padStart(12, "0")}`;
}
function source(
  method: "centralGet" | "routerPublicRead" | "retainedWriterRead" | "machineAuthentication",
) {
  return { method, artifact: `/private/fixture/${method}.json`, sha256: "a".repeat(64) };
}
function router(label: "sunbook" | "sunclaw", phase: "before" | "after"): RouterObservation {
  const installation = label === "sunbook" ? id(10) : id(11),
    project = label === "sunbook" ? id(1) : id(2),
    other = label === "sunbook" ? id(2) : id(1);
  const writer = {
    database: project,
    holder: label === "sunbook" ? id(20) : id(21),
    lease: 9007199254740993n,
  };
  return {
    machineLabel: label,
    machineId: writer.holder,
    installationId: installation,
    routerVersion: phase === "before" ? "0.1.125" : "0.1.135",
    build: {
      sourceHead: (phase === "before" ? "c" : "d").repeat(40),
      router: {
        path: "/fixture/router",
        sha256: "1".repeat(64),
        identifier: "test.router",
        team: "test",
      },
      cli: { path: "/fixture/cli", sha256: "2".repeat(64), identifier: "test.cli", team: "test" },
    },
    sources: [source("routerPublicRead"), source("retainedWriterRead")],
    listsComplete: true,
    writerReadComplete: true,
    workspaces: [{ id: id(30), facts: { name: "workspace", state: "active" } }],
    boards: [{ id: id(31), facts: { name: "board", workspaceId: id(30), state: "open" } }],
    workspaceAdrs: [
      { id: id(33), facts: { workspace: id(30), text: "literal ADR", status: "accepted" } },
    ],
    workspaceGraphComplete: true,
    folders: [
      { id: id(1), facts: { title: "one", board: id(31) } },
      { id: id(2), facts: { title: "two", board: id(31) } },
    ],
    writers: [writer],
    content: [
      {
        project: other,
        readerInstallation: installation,
        holderInstallation: label === "sunbook" ? id(11) : id(10),
        complete: true,
        records: [{ id: id(32), facts: { body: "literal project content", revision: 1n } }],
        source: source("routerPublicRead"),
      },
    ],
    authentication:
      phase === "before"
        ? { kind: "notObserved" }
        : {
            kind: "observed",
            connected: true,
            enrollInvocations: 0n,
            renewed: [writer],
            source: source("machineAuthentication"),
          },
  };
}
function observation(phase: "before" | "after"): Snapshot {
  return snapshot.parse({
    format: "central-inplace-observation-v1",
    runId: randomUUIDv7(),
    phase,
    routerProjects: [id(1), id(2)],
    routerTarget: {
      sourceHead: (phase === "before" ? "c" : "d").repeat(40),
      version: phase === "before" ? "0.1.125" : "0.1.135",
    },
    central: {
      origin: "https://central.example.test",
      sourceHead: (phase === "before" ? "a" : "b").repeat(40),
    },
    capturedAt: "2026-11-09T00:00:00Z",
    directory: [1, 2].map((index) => ({
      project: id(index),
      name: `project${index}`,
      workspace: id(30),
      board: id(31),
      boardArchive: "open",
      copySide: { kind: "notYetReported" },
      leaseSide: {
        kind: "known",
        value: {
          leaseVersion: 1n,
          asOf: "2026-11-09T00:00:00Z",
          lease: {
            kind: "held",
            value: {
              epoch: 9007199254740993n,
              holder: id(19 + index),
              state: "held",
              expiresAt: "2026-11-10T00:00:00Z",
            },
          },
        },
      },
    })),
    copies: [1, 2].map((index) => ({
      project: id(index),
      health: {
        kind: "live",
        value: {
          bookkeeping: [],
          migrationFailures: [],
          stage: { kind: "known", value: "following" },
          copy: {
            kind: "known",
            value: {
              database: id(index),
              role: "authoritative",
              fence: 9007199254740993n,
              holder: id(19 + index),
              start: { epoch: 1n, sequence: 2n },
              acked: { epoch: 1n, sequence: 7n },
              schema: { version: 3n, fingerprint: "schema-only" },
            },
          },
          projectFingerprint: { kind: "known", value: "schema-only" },
          stagedParts: [],
          storageBytes: 100n,
        },
      },
    })),
    sources: [source("centralGet")],
    routers: [router("sunbook", phase), router("sunclaw", phase)],
    limitations: ["registry-only/orphan rows require second-line inventory/decoder/S4"],
  });
}

test("same protected facts survive version change, key order and record-list order", (): void => {
  const before = observation("before"),
    after = observation("after");
  after.directory.reverse();
  after.copies.reverse();
  after.routers.reverse();
  assert.equal(compareSnapshots(before, after).status, "preserved");
  assert.deepEqual(canonical({ z: 1n, a: { y: 2n, x: 3n } }), { a: { x: 3n, y: 2n }, z: 1n });
});

test("declared source/version govern the new target and ADR changes cannot pass preservation", (): void => {
  const before = observation("before"),
    after = observation("after");
  after.routerTarget.version = "0.1.136";
  for (const machine of after.routers) machine.routerVersion = "0.1.136";
  assert.equal(compareSnapshots(before, after).status, "preserved");
  const first = after.routers[0];
  assert.ok(first);
  first.build.sourceHead = "e".repeat(40);
  assert.equal(compareSnapshots(before, after).status, "incomplete");
  first.build.sourceHead = after.routerTarget.sourceHead;
  first.workspaceGraphComplete = false;
  assert.equal(compareSnapshots(before, after).status, "incomplete");
  first.workspaceGraphComplete = true;
  const decision = first.workspaceAdrs[0];
  assert.ok(decision);
  decision.facts.text = "lost original choice";
  const changed = compareSnapshots(before, after);
  assert.equal(changed.status, "changed");
  assert.ok(
    changed.differences.some((difference): boolean => difference.rule === "R5 workspaceAdrs"),
  );
});

function nonHeld(observed: Snapshot, lifecycle: "unheld" | "retired"): void {
  const entry = observed.directory.find((row) => row.project === id(1));
  const copy = observed.copies.find((row) => row.project === id(1));
  assert.ok(entry?.leaseSide.kind === "known" && copy?.health.kind === "live");
  entry.leaseSide.value.lease =
    lifecycle === "unheld"
      ? { kind: "unheld" }
      : { kind: "retired", value: { retiredAt: "2026-11-08T00:00:00Z" } };
  if (lifecycle === "retired")
    copy.health = { kind: "retired", value: { retiredAt: "2026-11-08T00:00:00Z" } };
  else {
    assert.ok(copy.health.value.copy.kind === "known" && copy.health.value.copy.value !== null);
    copy.health.value.copy.value.holder = null;
    copy.health.value.copy.value.fence = 1n;
  }
  for (const router of observed.routers) {
    router.writers = router.writers.filter((row) => row.database !== id(1));
    if (router.authentication.kind === "observed")
      router.authentication.renewed = router.authentication.renewed.filter(
        (row) => row.database !== id(1),
      );
    if (lifecycle === "retired")
      router.content = router.content.filter((row) => row.project !== id(1));
    else
      for (const view of router.content) if (view.project === id(1)) view.holderInstallation = null;
  }
}
for (const lifecycle of ["unheld", "retired"] as const)
  test(`R1 preserves ${lifecycle} without inventing a local writer or renewal`, () => {
    const before = observation("before"),
      after = observation("after");
    nonHeld(before, lifecycle);
    nonHeld(after, lifecycle);
    assert.equal(compareSnapshots(before, after).status, "preserved");
  });
test("R1 catches a Registry-only lifecycle change even when Copy and native tuples do not change", () => {
  const before = observation("before"),
    after = observation("after");
  nonHeld(before, "unheld");
  nonHeld(after, "unheld");
  const entry = after.directory.find((row) => row.project === id(1));
  assert.ok(entry?.leaseSide.kind === "known");
  entry.leaseSide.value.lease = {
    kind: "held",
    value: { epoch: 1n, holder: id(20), state: "held", expiresAt: "2026-11-10T00:00:00Z" },
  };
  const result = compareSnapshots(before, after);
  assert.equal(result.status, "changed");
  assert.ok(result.differences.some((row) => row.rule === "R1 Registry lease"));
});
test("unreported Registry leases and contradictory held tuples cannot pass identical before/after", () => {
  const before = observation("before"),
    after = observation("after");
  for (const observed of [before, after]) {
    const [entry] = observed.directory;
    assert.ok(entry);
    entry.leaseSide = { kind: "notYetReported" };
  }
  assert.equal(compareSnapshots(before, after).status, "incomplete");
  const left = observation("before"),
    right = observation("after");
  for (const observed of [left, right]) {
    const [router] = observed.routers;
    assert.ok(router?.writers[0]);
    router.writers[0].lease += 1n;
  }
  assert.equal(compareSnapshots(left, right).status, "incomplete");
});
test("ordinary expiry and publication metadata do not change the preserved held grant", () => {
  const before = observation("before"),
    after = observation("after");
  const [entry] = after.directory;
  assert.ok(entry?.leaseSide.kind === "known" && entry.leaseSide.value.lease.kind === "held");
  entry.leaseSide.value.lease.value.state = "expired";
  entry.leaseSide.value.lease.value.expiresAt = "2026-11-11T00:00:00Z";
  entry.leaseSide.value.leaseVersion = 2n;
  assert.equal(compareSnapshots(before, after).status, "preserved");
});
for (const dimension of [
  "fence",
  "holder",
  "role",
  "start",
  "acked",
  "schema",
  "fingerprint",
] as const) {
  test(`R3 changed ${dimension} fails even when all other observations match`, (): void => {
    const before = observation("before"),
      after = observation("after"),
      copy = after.copies[0];
    assert.ok(copy?.health.kind === "live");
    const health = copy.health.value;
    assert.ok(health.copy.kind === "known" && health.copy.value !== null);
    const status = health.copy.value;
    switch (dimension) {
      case "fence":
        status.fence += 1n;
        break;
      case "holder":
        status.holder = id(90);
        break;
      case "role":
        status.role = "follower";
        break;
      case "start":
        status.start.sequence = 0n;
        break;
      case "acked":
        status.acked.sequence = 0n;
        break;
      case "schema":
        status.schema.version += 1n;
        break;
      case "fingerprint":
        health.projectFingerprint = { kind: "known", value: "changed" };
        break;
    }
    const result = compareSnapshots(before, after);
    assert.equal(result.status, "changed");
    assert.ok(result.differences.some((difference): boolean => difference.rule === "R3 copy"));
  });
}
test("R1 lease differences beyond Number safe range cannot be rounded away", (): void => {
  const before = observation("before"),
    after = observation("after"),
    writer = after.routers[0]?.writers[0];
  assert.ok(writer);
  writer.lease += 1n;
  assert.ok(
    compareSnapshots(before, after).differences.some(
      (difference): boolean => difference.rule === "R1 writer",
    ),
  );
});
test("R3 row content changes fail despite identical schema fingerprint and positions", (): void => {
  const before = observation("before"),
    after = observation("after"),
    row = after.routers[0]?.content[0]?.records[0];
  assert.ok(row);
  row.facts.body = "lost original content";
  assert.ok(
    compareSnapshots(before, after).differences.some(
      (difference): boolean => difference.rule === "R3 content",
    ),
  );
});
for (const field of ["workspaces", "boards", "folders"] as const) {
  test(`R5 changed ${field} facts fail`, (): void => {
    const before = observation("before"),
      after = observation("after"),
      row = after.routers[0]?.[field][0];
    assert.ok(row);
    row.facts.state = "archived";
    assert.equal(compareSnapshots(before, after).status, "changed");
  });
}
test("R6 a newly enrolled machine is a difference, not a preserved installation", (): void => {
  const before = observation("before"),
    after = observation("after"),
    machine = after.routers[0];
  assert.ok(machine);
  machine.machineId = id(99);
  assert.ok(
    compareSnapshots(before, after).differences.some(
      (difference): boolean => difference.rule === "R6 machine",
    ),
  );
});
test("missing Router, writer, content or authentication cannot turn equality into pass", (): void => {
  const before = observation("before");
  for (const missing of ["router", "writer", "content", "authentication"] as const) {
    const after = observation("after"),
      machine = after.routers[0];
    assert.ok(machine);
    if (missing === "router") after.routers.pop();
    if (missing === "writer") {
      machine.writers = [];
      machine.writerReadComplete = false;
    }
    if (missing === "content") machine.content = [];
    if (missing === "authentication") machine.authentication = { kind: "notObserved" };
    const result = compareSnapshots(before, after);
    assert.notEqual(result.status, "preserved");
    assert.ok(result.coverageGaps.length > 0);
  }
});
test("missing or mismatching same-lease renewal remains incomplete", (): void => {
  const after = observation("after"),
    auth = after.routers[0]?.authentication;
  assert.ok(auth?.kind === "observed");
  auth.renewed = [];
  assert.equal(compareSnapshots(observation("before"), after).status, "incomplete");
});
test("duplicate identities, empty snapshots and holder-as-reader evidence are refused", (): void => {
  const before = observation("before"),
    after = observation("after");
  const first = after.directory[0];
  assert.ok(first);
  after.directory.push(first);
  assert.throws((): void => {
    compareSnapshots(before, after);
  }, /duplicate/);
  assert.throws((): void => {
    snapshot.parse({ ...before, directory: [] });
  });
  const content = before.routers[0]?.content[0];
  assert.ok(content);
  content.holderInstallation = content.readerInstallation;
  assert.throws((): void => {
    snapshot.parse(before);
  });
});
test("collector follows pages through the explicit empty terminal page and reads each health once", async (): Promise<void> => {
  const expected = observation("before"),
    paths: string[] = [];
  const read: CentralRead = async (path) => {
    paths.push(path);
    const project = path.split("/").at(-2),
      health = expected.copies.find((copy): boolean => copy.project === project)?.health;
    return {
      artifact: `/fixture/${paths.length}.json`,
      sha256: "b".repeat(64),
      body: path.includes("/health")
        ? health
        : path.includes("after=")
          ? { projects: [], nextAfter: null, asOf: "2026-11-09T00:00:00Z" }
          : { projects: expected.directory, nextAfter: id(2), asOf: "2026-11-09T00:00:00Z" },
    };
  };
  const result = await collectCentral({
    plan: {
      centralOrigin: expected.central.origin,
      sourceHead: expected.central.sourceHead,
      routerTarget: expected.routerTarget,
      routerProjects: expected.routerProjects,
      phase: "before",
      evidenceDirectory: "/fixture",
      ownerAssertionEnv: "PRIVATE_OWNER_ASSERTION",
      routerObservationFiles: [],
    },
    read,
  });
  assert.equal(paths.length, 4);
  assert.deepEqual(result.directory, expected.directory);
  assert.equal(result.copies.length, 2);
  assert.equal(compareSnapshots(result, observation("after")).status, "changed");
});
test("collector refuses repeating cursors, empty inventory and a different health database", async (): Promise<void> => {
  const fixture = observation("before");
  const plan = {
    centralOrigin: fixture.central.origin,
    sourceHead: fixture.central.sourceHead,
    routerTarget: fixture.routerTarget,
    routerProjects: fixture.routerProjects,
    phase: "before",
    evidenceDirectory: "/fixture",
    ownerAssertionEnv: "PRIVATE_OWNER_ASSERTION",
    routerObservationFiles: [],
  } as const;
  await assert.rejects(
    collectCentral({
      plan: { ...plan, routerObservationFiles: [] },
      read: async () => ({
        artifact: "/fixture/page",
        sha256: "c".repeat(64),
        body: { projects: [], nextAfter: null, asOf: "now" },
      }),
    }),
    /empty/,
  );
  await assert.rejects(
    collectCentral({
      plan: { ...plan, routerObservationFiles: [] },
      read: async () => ({
        artifact: "/fixture/page",
        sha256: "c".repeat(64),
        body: { projects: fixture.directory, nextAfter: id(2), asOf: "now" },
      }),
    }),
    /duplicate|nonadvancing/,
  );
  const copy = fixture.copies[0];
  assert.ok(
    copy?.health.kind === "live" &&
      copy.health.value.copy.kind === "known" &&
      copy.health.value.copy.value,
  );
  copy.health.value.copy.value.database = id(99);
  await assert.rejects(
    collectCentral({
      plan: { ...plan, routerObservationFiles: [] },
      read: async (path) => ({
        artifact: "/fixture/page",
        sha256: "c".repeat(64),
        body: path.includes("health")
          ? copy.health
          : { projects: fixture.directory, nextAfter: null, asOf: "now" },
      }),
    }),
    /database differs/,
  );
});

test("selected Router projects stay strict while unopened central copies retain full preservation coverage", (): void => {
  const before = observation("before"),
    after = observation("after");
  for (const captured of [before, after]) {
    captured.routerProjects = [id(1)];
    for (const machine of captured.routers) {
      machine.writers = machine.writers.filter((writer) => writer.database === id(1));
      machine.content = machine.content.filter((view) => view.project === id(1));
      if (machine.authentication.kind === "observed")
        machine.authentication.renewed = machine.authentication.renewed.filter(
          (writer) => writer.database === id(1),
        );
    }
  }
  assert.equal(compareSnapshots(before, after).status, "preserved");
  const unopened = after.copies.find((copy) => copy.project === id(2));
  assert.ok(
    unopened?.health.kind === "live" &&
      unopened.health.value.copy.kind === "known" &&
      unopened.health.value.copy.value !== null,
  );
  unopened.health.value.copy.value.acked.sequence += 1n;
  assert.equal(compareSnapshots(before, after).status, "changed");
  unopened.health.value.copy.value.acked.sequence -= 1n;
  after.routers.forEach((machine): void => {
    machine.writers = [];
  });
  const missingWriter = compareSnapshots(before, after);
  assert.notEqual(missingWriter.status, "preserved");
  assert.ok(
    missingWriter.coverageGaps.some((gap): boolean => gap.includes("current native writer")),
  );
});
