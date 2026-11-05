/** A real first Luna turn establishes the recipient before a separate mail observation. */
import { setTimeout } from "node:timers/promises";
import { z } from "zod";
import type { D1Machine, D1HarnessDeclaration } from "./d1-harness.ts";
import type { Receipt } from "../../receipt.ts";
import { boardIdentity } from "../../clients/board-requests.ts";
import { field } from "../../exact-json.ts";
import { reserveD1Turn } from "./d1-turn-budget.ts";
import { senderStage, unloadD1Recipient } from "./d1-mail-stages.ts";

interface MaterializedDrive {
  declaration: D1HarnessDeclaration;
  machines: readonly D1Machine[];
  receipt: Receipt;
}

function participants(input: MaterializedDrive): { sender: D1Machine; recipient: D1Machine } {
  const sender = input.machines[0],
    recipient = input.machines[1];
  if (
    sender === undefined ||
    recipient === undefined ||
    input.declaration.materialization === undefined
  )
    throw new Error("declared materialized B recipient required");
  return { sender, recipient };
}

async function reserve(
  input: MaterializedDrive,
  reason: "materialize" | "release-held",
): Promise<void> {
  const { recipient } = participants(input);
  const grant = input.declaration.materialization;
  if (grant === undefined) throw new Error("D1 model grant absent");
  const reservation = await reserveD1Turn({
    repository: input.declaration.repository,
    budgetFile: grant.budgetFile,
    session: recipient.recipient.sessionId,
    reason,
  });
  await input.receipt.record("d1TurnReserved", {
    reservation,
    reason,
    recipient: recipient.recipient,
    model: "gpt-6-luna",
    effort: "low",
    origin: input.declaration.debugProxyOrigin,
  });
}

async function completedTurn(input: MaterializedDrive, turnId: string): Promise<void> {
  const { recipient } = participants(input);
  const deadline = Date.now() + 90_000;
  for (;;) {
    recipient.native.healthy();
    for (const event of recipient.native.events.filter(
      (event): boolean => event.method === "turn/completed",
    )) {
      const completed = z
        .object({
          threadId: z.string(),
          turn: z.object({ id: z.string(), status: z.string(), error: z.unknown().optional() }),
        })
        .parse(event.params);
      if (completed.threadId !== recipient.recipient.sessionId || completed.turn.id !== turnId)
        continue;
      await input.receipt.record("d1MaterializationTurnFinished", {
        thread: completed.threadId,
        turn: turnId,
        status: completed.turn.status,
      });
      if (completed.turn.status !== "completed")
        throw new Error(
          "D1 first/released turn failed; stop all model input and report auth/quota or native failure",
        );
      return;
    }
    if (Date.now() >= deadline) throw new Error("D1 turn completion unconfirmed; stop model input");
    await setTimeout(25);
  }
}

async function materialize(input: MaterializedDrive): Promise<void> {
  const { recipient } = participants(input);
  await reserve(input, "materialize");
  const response = await recipient.native.rpc("turn/start", {
    threadId: recipient.recipient.sessionId,
    model: "gpt-6-luna",
    effort: "low",
    input: [
      {
        type: "text",
        text: "Reply with READY only. Do not use tools, access files, send messages or delegate.",
      },
    ],
  });
  const turn = z.object({ turn: z.object({ id: z.string().min(1) }) }).parse(response).turn;
  await input.receipt.record("d1MaterializationTurnStarted", {
    thread: recipient.recipient.sessionId,
    turn: turn.id,
    model: "gpt-6-luna",
    effort: "low",
  });
  await completedTurn(input, turn.id);
  await unloadD1Recipient(recipient, input.receipt);
  const stored = await recipient.native.rpc("thread/read", {
    threadId: recipient.recipient.sessionId,
    includeTurns: false,
  });
  z.object({ thread: z.object({ id: z.literal(recipient.recipient.sessionId) }) }).parse(stored);
  await input.receipt.record("d1MaterializedRecipientStored", {
    recipient: recipient.recipient,
    turn: turn.id,
  });
}

async function requireCounts(input: MaterializedDrive, expectedB: number): Promise<void> {
  const counts = input.machines.map((machine, index) => ({
    service: machine.recipient.endpoint.serviceId,
    observed: machine.nativeTurnCount(),
    expected: index === 1 ? expectedB : 0,
  }));
  await input.receipt.record("d1BoundedTurnObservation", { counts });
  if (counts.some((count): boolean => count.observed !== count.expected))
    throw new Error("D1 actual turn count differs from its reserved stage");
}

async function reloadHeld(input: MaterializedDrive, item: string): Promise<void> {
  const { sender, recipient } = participants(input);
  await reserve(input, "release-held");
  const loadStartedAt = Date.now();
  const resumed = await recipient.native.rpc("thread/resume", {
    threadId: recipient.recipient.sessionId,
    model: "gpt-6-luna",
    modelProvider: "codex-router-debug",
    config: { model_reasoning_effort: "low", thread_unload_delay_secs: 0 },
  });
  z.object({
    model: z.literal("gpt-6-luna"),
    modelProvider: z.literal("codex-router-debug"),
    reasoningEffort: z.literal("low"),
  }).parse(resumed);
  await senderStage({
    sender,
    item,
    kinds: ["delivered"],
    receipt: input.receipt,
    stage: "materialized-delivered",
    timeoutMilliseconds: 90_000,
  });
  await input.receipt.record("d1HeldReleaseObserved", {
    item,
    elapsedMilliseconds: Date.now() - loadStartedAt,
    status: "delivered",
  });
  const deadline = Date.now() + 90_000;
  let turnId: string | undefined;
  while (turnId === undefined) {
    const started = recipient.native.events.filter(
      (event): boolean => event.method === "turn/started",
    );
    if (started.length > 2) throw new Error("unexpected extra turn during Held release");
    if (started.length === 2) turnId = z.string().parse(field(started[1]?.params, "turn", "id"));
    else {
      if (Date.now() >= deadline)
        throw new Error("delivered mail did not expose its real queued turn");
      await setTimeout(25);
    }
  }
  await completedTurn(input, turnId);
  await requireCounts(input, 2);
}

export async function driveMaterializedD1Mail(
  input: MaterializedDrive,
): Promise<readonly string[]> {
  const { sender, recipient } = participants(input);
  await requireCounts(input, 0);
  await materialize(input);
  await requireCounts(input, 1);
  const timing = input.declaration.queueScenario === "online-timing";
  const items: string[] = [];
  for (let sample = 0; sample < (timing ? 20 : 1); sample += 1) {
    const item = boardIdentity();
    const request = {
      itemId: item,
      target: recipient.recipient,
      delivery: "queue",
      message: {
        kind: "agent",
        sender: sender.recipient,
        text: `Reply with READY only. Do not use tools. D1 held fixture ${item}.`,
      },
    };
    const submitted = await sender.mcp.call("message_send", request);
    z.object({ queued: z.object({ item: z.literal(item) }) }).parse(submitted);
    await input.receipt.record("d1MaterializedMailAdmission", { item, submitted, sample });
    await senderStage({
      sender,
      item,
      kinds: ["held"],
      receipt: input.receipt,
      stage: `materialized-held-${sample}`,
    });
    const shown = await recipient.mcp.call("router_show", {
      caller: recipient.recipient,
      reference: item,
    });
    z.object({
      record: z.object({ pushId: z.literal(item), deliveryState: z.literal("held") }),
    }).parse(shown);
    await input.receipt.record("d1MaterializedRecipientHeld", { item, shown });
    await requireCounts(input, 1);
    items.push(item);
  }
  const first = items[0];
  if (!timing && input.declaration.materialization?.reloadHeld === true && first !== undefined)
    await reloadHeld(input, first);
  await input.receipt.save("materialized-items.json", {
    items,
    timing,
    reloadHeld: !timing && input.declaration.materialization?.reloadHeld,
  });
  return timing ? items : [];
}
