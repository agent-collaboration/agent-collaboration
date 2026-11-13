// Verification O1, storage boundary basics, on the Cloudflare host: each case runs
// core's widgets schema and statements inside a real SQLite-backed Durable Object of
// the central Worker built with its test-support feature. Expected values come from
// Program Design #storage-boundary; the Worker only executes the steps.
import assert from "node:assert/strict";
import { z } from "zod";
import { after, before, describe, test } from "node:test";
import { WorkerdStorage, centralTestBundle } from "./workerd-storage.ts";

type Migrator = "widgets" | "failingSecondStatement";
type SqlValue =
  | { kind: "null" }
  | { kind: "integer"; value: number }
  | { kind: "text"; value: string };
type Step =
  | { insertOwner: { ownerId: string } }
  | {
      insertWidget: {
        widgetId: string;
        ownerId: string;
        name: SqlValue;
        active: SqlValue;
        dueAtMs: number | null;
      };
    }
  | { retireLiveWidget: { widgetId: string } };
type Alarm = "unchanged" | "earliestWidgetDue";

const text = (value: string): SqlValue => ({ kind: "text", value });
const integer = (value: number): SqlValue => ({ kind: "integer", value });
const owner = (ownerId: string): Step => ({ insertOwner: { ownerId } });
const widget = (
  widgetId: string,
  ownerId: string,
  name: SqlValue = text(widgetId),
  active: SqlValue = integer(1),
  dueAtMs: number | null = null,
): Step => ({ insertWidget: { widgetId, ownerId, name, active, dueAtMs } });

// Far enough ahead that the alarm never fires while a case runs.
const FAR_ALARM_MS = 4_102_444_800_000;
const WORKSPACE = "019f0000-0000-7000-8000-0000000000a1";

let central: WorkerdStorage;

async function call(
  object: string,
  operation: string,
  body: unknown,
  migrator: Migrator = "widgets",
): Promise<Response> {
  return fetch(`${central.origin}/test-support/storage/${object}/${operation}`, {
    method: "POST",
    headers: { "x-case-migrator": JSON.stringify(migrator) },
    body: JSON.stringify(body ?? null),
  });
}

async function json(
  object: string,
  operation: string,
  body: unknown,
  migrator?: Migrator,
): Promise<unknown> {
  const response = await call(object, operation, body, migrator);
  assert.equal(response.status, 200, await response.clone().text());
  return response.json();
}

const batch = (object: string, steps: Step[], alarm: Alarm = "unchanged"): Promise<unknown> =>
  json(object, "batch", { steps, alarm });
const read = (object: string, what: string, migrator?: Migrator): Promise<unknown> =>
  json(object, "read", what, migrator);
type TaggedValue = { kind: string; value?: unknown };
const plain = (row: TaggedValue[]): unknown[] => row.map((cell) => cell.value ?? null);
const rows = async (object: string, what: string, migrator?: Migrator): Promise<unknown[][]> =>
  ((await read(object, what, migrator)) as { Ok: { rows: TaggedValue[][] } }).Ok.rows.map(plain);
const applied = (rowsAffected: number[]): unknown => ({ Ok: { outcome: "applied", rowsAffected } });
const violated = (index: number, kind: string): unknown => ({
  Ok: { outcome: "constraintViolated", index, kind },
});

before(async () => {
  central = await WorkerdStorage.start(centralTestBundle());
});
after(async () => {
  await central.close();
});

describe("statement batches on a Durable Object", () => {
  test("a failed guard on the second statement rolls the first back", async () => {
    assert.deepEqual(await batch("atomic", [owner("o1")]), applied([1]));
    assert.deepEqual(
      await batch("atomic", [widget("w1", "o1"), { retireLiveWidget: { widgetId: "missing" } }]),
      {
        Ok: { outcome: "guardFailed", index: 1, rowsAffected: 0 },
      },
    );
    assert.deepEqual(await rows("atomic", "listWidgets"), []);
  });

  test("unique, foreign key, not null and boolean check map to their constraint kinds", async () => {
    assert.deepEqual(
      await batch("constraints", [owner("o1"), widget("w1", "o1", text("same"))]),
      applied([1, 1]),
    );
    assert.deepEqual(
      await batch("constraints", [widget("w2", "o1", text("same"))]),
      violated(0, "unique"),
    );
    // Foreign keys are enforced in the Durable Object (the Program Design experiment).
    assert.deepEqual(
      await batch("constraints", [widget("w3", "nobody")]),
      violated(0, "foreignKey"),
    );
    assert.deepEqual(
      await batch("constraints", [widget("w4", "o1", { kind: "null" })]),
      violated(0, "notNull"),
    );
    assert.deepEqual(
      await batch("constraints", [widget("w5", "o1", text("w5"), integer(2))]),
      violated(0, "checkBoolean"),
    );
    // The unique index is partial: a retired name frees it.
    assert.deepEqual(
      await batch("constraints", [
        { retireLiveWidget: { widgetId: "w1" } },
        widget("w6", "o1", text("same")),
      ]),
      applied([1, 1]),
    );
    assert.deepEqual(
      (await rows("constraints", "listWidgets")).map((row) => row[0]),
      ["w1", "w6"],
    );
  });

  test("SetFromQuery sets the alarm in the batch's transaction and NULL clears it", async () => {
    assert.deepEqual(
      await batch(
        "alarm",
        [owner("o1"), widget("w1", "o1", text("w1"), integer(1), FAR_ALARM_MS)],
        "earliestWidgetDue",
      ),
      applied([1, 1]),
    );
    assert.deepEqual(await json("alarm", "alarm", null), { alarmMs: FAR_ALARM_MS });
    assert.deepEqual(
      await batch("alarm", [{ retireLiveWidget: { widgetId: "w1" } }], "earliestWidgetDue"),
      applied([1]),
    );
    assert.deepEqual(await json("alarm", "alarm", null), { alarmMs: null });
    // An alarm written inside a storage transaction that then fails is rolled back.
    assert.deepEqual(await json("alarm", "alarm-rollback", { atMs: FAR_ALARM_MS }), {
      transactionFailed: true,
      alarmMs: null,
    });
  });

  test("ids minted in one batch keep their order while the clock is held", async () => {
    const result = (await json("ids", "mint-ordered", {
      count: 32,
      heldAtMs: 1_800_000_000_000,
      ownerId: "o1",
    })) as { minted: string[]; outcome: unknown };
    assert.deepEqual(result.outcome, applied([1, ...result.minted.map(() => 1)]));
    assert.equal(
      new Set(result.minted.map((id) => id.slice(0, 13))).size,
      1,
      "one held millisecond",
    );
    assert.deepEqual(
      (await rows("ids", "listWidgets")).map((row) => row[0]),
      result.minted,
    );
  });
});

describe("integers within ±(2^53−1)", () => {
  const MAX_SAFE = 9_007_199_254_740_991;
  const rawRows = async (object: string): Promise<TaggedValue[][]> =>
    ((await read(object, "listWidgets")) as { Ok: { rows: TaggedValue[][] } }).Ok.rows;

  test("the safe extremes are stored and read back exactly", async () => {
    assert.deepEqual(
      await batch("integers", [
        owner("o1"),
        widget("max", "o1", text("max"), integer(1), MAX_SAFE),
        widget("min", "o1", text("min"), integer(1), -MAX_SAFE),
      ]),
      applied([1, 1, 1]),
    );
    const byId = new Map((await rawRows("integers")).map((row) => [row[0]?.value, row]));
    assert.deepEqual(byId.get("max")?.[5], { kind: "integer", value: MAX_SAFE });
    assert.deepEqual(byId.get("min")?.[5], { kind: "integer", value: -MAX_SAFE });
    // An INTEGER column holding a whole number reads back as an integer.
    assert.deepEqual(byId.get("max")?.[3], { kind: "integer", value: 1 });
  });

  test("2^53 is refused on the wire and nothing is stored", async () => {
    const response = await call("integers-refused", "batch", {
      steps: [owner("o1"), widget("too-big", "o1", text("too-big"), integer(1), MAX_SAFE + 1)],
      alarm: "unchanged",
    });
    assert.equal(response.status, 422);
    assert.match(((await response.json()) as { refused: string }).refused, /2\^53/);
    assert.deepEqual(await rows("integers-refused", "countOwners"), [[0]]);
    const literal = await call("integers-refused", "batch", {
      steps: [
        owner("o2"),
        {
          insertWidget: {
            widgetId: "w",
            ownerId: "o2",
            name: text("w"),
            active: { kind: "integer", value: MAX_SAFE + 1 },
            dueAtMs: null,
          },
        },
      ],
      alarm: "unchanged",
    });
    assert.equal(literal.status, 422);
    assert.deepEqual(await rows("integers-refused", "countOwners"), [[0]]);
  });
});

describe("migrations at object open", () => {
  test("a migration failing on its second statement leaves no partial schema and the history unchanged", async () => {
    assert.deepEqual(await json("migrations", "open", null), { opened: true });
    const historyBefore = await read("migrations", "listAppliedMigrations");
    assert.equal((historyBefore as { Ok: { rows: unknown[] } }).Ok.rows.length, 1);
    // Restart the object so the next request opens it again, with the failing source.
    assert.notEqual((await call("migrations", "crash", null)).status, 200);
    const reopened = (await json("migrations", "open", null, "failingSecondStatement")) as {
      opened: boolean;
      error?: string;
    };
    assert.equal(reopened.opened, false);
    assert.match(reopened.error ?? "", /migration 2 failed/);
    assert.notEqual((await call("migrations", "crash", null)).status, 200);
    const tables = (await rows("migrations", "listTables")).map((row) => row[0]);
    assert.ok(tables.includes("widgets"));
    assert.ok(!tables.includes("widget_labels"), "the failing migration's first table rolled back");
    // Every column of every receipt is as it was before the failing migration.
    assert.deepEqual(await read("migrations", "listAppliedMigrations"), historyBefore);
    assert.deepEqual(
      (await rows("migrations", "listAppliedMigrations")).map((row) => [row[0], row[3]]),
      [[1, 1]],
    );
  });
});

describe("call_store reaches the workspace's store object", () => {
  const callStore = async (body: unknown): Promise<unknown> => {
    const response = await fetch(`${central.origin}/test-support/call-store/${WORKSPACE}`, {
      method: "POST",
      body: JSON.stringify(body),
    });
    assert.equal(response.status, 200, await response.clone().text());
    return response.json();
  };

  test("a batch and a read cross to the store and back", async () => {
    assert.deepEqual(
      await callStore({ batch: { steps: [owner("o1"), widget("w1", "o1")], alarm: "unchanged" } }),
      {
        Ok: { kind: "batch", reply: { outcome: "applied", rowsAffected: [1, 1] } },
      },
    );
    assert.deepEqual(
      await callStore({ batch: { steps: [widget("w2", "nobody")], alarm: "unchanged" } }),
      {
        Ok: {
          kind: "batch",
          reply: { outcome: "constraintViolated", index: 0, kind: "foreignKey" },
        },
      },
    );
    const listed = (await callStore({ read: "listWidgets" })) as {
      Ok: { kind: string; reply: { columns: string[]; rows: TaggedValue[][] } };
    };
    assert.equal(listed.Ok.kind, "read");
    assert.deepEqual(listed.Ok.reply.columns.slice(0, 3), ["widget_id", "owner_id", "name"]);
    assert.deepEqual(
      listed.Ok.reply.rows.map((row) => plain(row)[0]),
      ["w1"],
    );
  });
});

describe("the pause seam inside a Durable Object", () => {
  const target = (point: string): unknown => ({ workspace: WORKSPACE, point });
  const request = (ownerId: string): Promise<Response> =>
    call("seam", "request", {
      workspace: WORKSPACE,
      batch: { steps: [owner(ownerId)], alarm: "unchanged" },
    });

  test("a held request keeps the turn, a second waits for it, and both apply after release", async () => {
    await json("seam", "hold", target("afterReads"));
    const first = request("a");
    assert.deepEqual(await json("seam", "until-held", target("afterReads")), {
      held: ["afterReads"],
      waiting: 0,
    });
    const second = request("b");
    assert.deepEqual(await json("seam", "until-waiting", { workspace: WORKSPACE, count: 1 }), {
      held: ["afterReads"],
      waiting: 1,
    });
    await json("seam", "release", target("afterReads"));
    for (const answer of await Promise.all([first, second])) {
      assert.equal(answer.status, 200);
      assert.deepEqual(((await answer.json()) as { outcome: unknown }).outcome, applied([1]));
    }
    assert.deepEqual(await rows("seam", "countOwners"), [[2]]);
  });

  test("a crash after the store applied keeps the batch; a crash after the reads keeps nothing", async () => {
    await json("seam", "hold", target("afterStoreApplied"));
    const keptRequest = request("c");
    await json("seam", "until-held", target("afterStoreApplied"));
    assert.notEqual((await call("seam", "crash", null)).status, 200);
    assert.notEqual((await keptRequest).status, 200, "the held request went with the object");
    assert.deepEqual(await rows("seam", "countOwners"), [[3]]);

    await json("seam", "hold", target("afterReads"));
    const unapplied = request("d");
    await json("seam", "until-held", target("afterReads"));
    assert.notEqual((await call("seam", "crash", null)).status, 200);
    assert.notEqual((await unapplied).status, 200);
    assert.deepEqual(await rows("seam", "countOwners"), [[3]]);
  });
});

describe("the SQLx Durable Object driver's scenarios", () => {
  // Moved with the driver from agent-fleet-sync; each runs inside a real
  // SQLite-backed Durable Object and answers whether its own assertions held.

  test("every driver scenario passes", async () => {
    const listed: unknown = await (await fetch(`${central.origin}/test-support/driver/`)).json();
    assert.ok(Array.isArray(listed) && listed.length > 0, "the Worker lists no scenarios");
    const failures: string[] = [];
    // All of them, then one failure listing every scenario that failed.
    for (const name of listed as string[]) {
      const body = await (await fetch(`${central.origin}/test-support/driver/${name}`)).text();
      try {
        const outcome = JSON.parse(body) as { ok: boolean; error?: string };
        if (!outcome.ok) failures.push(`${name}: ${outcome.error ?? "no error given"}`);
      } catch {
        failures.push(`${name}: ${body}`);
      }
    }
    assert.deepEqual(failures, []);
  });
});

test("actual object ids follow held and advanced conformance time", async () => {
  const observation = z.object({ nowMs: z.number().int(), ids: z.array(z.string()).length(2) });
  for (const heldAtMs of [FAR_ALARM_MS, FAR_ALARM_MS + 123]) {
    const response = await fetch(`${central.origin}/test-support/object-clock-ids`, {
      method: "POST",
      body: JSON.stringify(new Date(heldAtMs).toISOString()),
    });
    assert.equal(response.status, 200, await response.clone().text());
    const result = observation.parse(await response.json());
    assert.equal(result.nowMs, heldAtMs);
    for (const id of result.ids) {
      assert.match(id, /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
      assert.equal(Number.parseInt(id.replaceAll("-", "").slice(0, 12), 16), heldAtMs);
    }
    assert.equal(new Set(result.ids).size, 2);
    assert.deepEqual(result.ids, [...result.ids].sort());
  }
});
