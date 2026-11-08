import { test } from "node:test";
import assert from "node:assert/strict";
import { settledMail } from "./compatibility-journeys.ts";

const recipient = {
  endpoint: { serviceId: "019eef00-0000-7000-8000-000000000001", endpointId: "codex-local" },
  sessionId: "019eef00-0000-7000-8000-000000000002",
};
const item = "019eef00-0000-7000-8000-000000000003";

test("mail custody waits through atMachine and requires the settled recipient record", async (): Promise<void> => {
  const statuses = ["atMachine", "held"];
  const records: { kind: string; value: unknown }[] = [];
  let recipientReads = 0;
  await settledMail({
    item,
    sender: {
      recipient,
      mcp: {
        async call(): Promise<unknown> {
          const status = statuses.shift();
          assert.ok(status, "waiter read beyond the declared custody sequence");
          return { mail: { status: { kind: status } } };
        },
      },
    },
    target: {
      recipient,
      mcp: {
        async call(): Promise<unknown> {
          recipientReads += 1;
          return { record: { pushId: item, deliveryState: "held" } };
        },
      },
    },
    receipt: {
      async record(kind: string, value: unknown): Promise<void> {
        records.push({ kind, value });
      },
    },
  });
  assert.deepEqual(statuses, []);
  assert.equal(recipientReads, 1);
  assert.deepEqual(records, [
    {
      kind: "d3MailReadback",
      value: {
        item,
        status: "held",
        shown: { mail: { status: { kind: "held" } } },
        received: { record: { pushId: item, deliveryState: "held" } },
      },
    },
  ]);
});

test("atMachine cannot hide a later refusal", async (): Promise<void> => {
  const statuses = ["atMachine", "refused"];
  const records: string[] = [];
  await assert.rejects(
    settledMail({
      item,
      sender: {
        recipient,
        mcp: {
          async call(): Promise<unknown> {
            const status = statuses.shift();
            assert.ok(status, "waiter read beyond the declared custody sequence");
            return { mail: { status: { kind: status } } };
          },
        },
      },
      target: {
        recipient,
        mcp: {
          async call(): Promise<unknown> {
            assert.fail("refused mail must not receive inbox credit");
          },
        },
      },
      receipt: {
        async record(kind: string): Promise<void> {
          records.push(kind);
        },
      },
    }),
    /old-source message refused or lost: refused/,
  );
  assert.deepEqual(statuses, []);
  assert.deepEqual(records, ["d3MailUnexpectedStatus"]);
});
