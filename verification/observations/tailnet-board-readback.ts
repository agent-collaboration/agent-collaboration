import { existsSync, lstatSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { z } from "zod";
import { readObservedJson } from "./evidence-models.ts";
import { saveRecord } from "./runtime-plan.ts";
import { requirePrivateDirectory } from "./tailnet-board-peer.ts";
import {
  boardService,
  requireDeadline,
  requireMarker,
  type BoardMarker,
  type BoardPlan,
} from "./tailnet-plans.ts";
import { requireService, tool } from "./tailnet-transport.ts";

export function readStoredMarker(database: string, marker: BoardMarker, actor: unknown): void {
  const connection = new DatabaseSync(database, { readOnly: true });
  let row: unknown;
  try {
    row = connection
      .prepare(
        "SELECT m.message_id,m.topic_id,m.text,i.kind,i.service_id,i.endpoint_id,i.session_id,m.root_id FROM board_messages m JOIN board_identities i ON i.identity_key=m.actor_key WHERE m.message_id=?",
      )
      .get(marker.message_id);
  } finally {
    connection.close();
  }
  const stored = z
    .object({
      message_id: z.string(),
      topic_id: z.string(),
      text: z.string(),
      kind: z.string(),
      service_id: z.string(),
      endpoint_id: z.string(),
      session_id: z.string(),
      root_id: z.string(),
    })
    .parse(row);
  requireMarker(
    {
      messageId: stored.message_id,
      topicId: stored.topic_id,
      placement: { kind: "thread", rootMessageId: stored.root_id },
      text: stored.text,
      actor: {
        kind: stored.kind,
        session: {
          endpoint: { serviceId: stored.service_id, endpointId: stored.endpoint_id },
          sessionId: stored.session_id,
        },
      },
    },
    marker,
    actor,
  );
}

export async function runReadback(plan: BoardPlan): Promise<void> {
  requireDeadline(plan.deadline);
  requirePrivateDirectory(plan.destination_root);
  requirePrivateDirectory(plan.evidence);
  const service = boardService(plan);
  const manifest = readObservedJson(readFileSync(join(service, "service.json")));
  if (
    manifest.serviceId !== plan.destination_service_id ||
    manifest.serviceEpoch !== plan.destination_service_epoch
  )
    throw new Error("independent destination manifest mismatch");
  requireService(
    await tool(join(service, "control.sock"), "endpoints_list", {}),
    plan.destination_service_id,
    plan.destination_service_epoch,
  );
  const observed = await tool(join(service, "control.sock"), "board_message_show", {
    messageId: plan.marker.message_id,
  });
  requireMarker(observed, plan.marker, plan.actor);
  const database = join(service, "project-board.sqlite");
  if (
    !existsSync(database) ||
    !statSync(database).isFile() ||
    lstatSync(database).isSymbolicLink() ||
    statSync(database).uid !== process.getuid?.()
  )
    throw new Error("destination board store binding absent");
  readStoredMarker(database, plan.marker, plan.actor);
  saveRecord(join(plan.evidence, "destination-readback.json"), {
    serviceId: plan.destination_service_id,
    store: database,
    readback: observed,
    restartPersistence: "not exercised",
  });
}
