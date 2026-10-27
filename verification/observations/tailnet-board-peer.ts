import { lstatSync, realpathSync } from "node:fs";
import { join } from "node:path";
import { z } from "zod";
import { sameJsonContent } from "../json-comparison.ts";
import { observedObject, type JsonObject } from "./evidence-models.ts";
import { errorCategory, saveRecord } from "./runtime-plan.ts";
import { httpsOrigin, requireDeadline, requireMarker, type BoardPlan } from "./tailnet-plans.ts";
import {
  httpPost,
  requireService,
  rpcResult,
  toolPayload,
  toolResult,
  type HttpReply,
  type HttpTarget,
  type ProbeHeaders,
} from "./tailnet-transport.ts";

export function requirePrivateDirectory(path: string): void {
  const metadata = lstatSync(path);
  if (
    realpathSync(path) !== path ||
    !metadata.isDirectory() ||
    metadata.uid !== process.getuid?.() ||
    (metadata.mode & 0o077) !== 0
  )
    throw new Error("evidence/destination directory is not owner-private and canonical");
}
export type PeerRequest = (
  target: HttpTarget,
  payload: JsonObject,
  headers?: ProbeHeaders | null,
) => Promise<HttpReply>;
interface PeerPreflight {
  readonly tools: JsonObject;
  readonly originStatuses: JsonObject;
  readonly threadRoot: JsonObject;
}

async function peerPreflight(plan: BoardPlan, request: PeerRequest): Promise<PeerPreflight> {
  const target: HttpTarget = { kind: "https", endpoint: plan.https };
  const requestTool = async (name: string, arguments_: JsonObject): Promise<JsonObject> =>
    toolResult(await request(target, toolPayload(name, arguments_), null));
  requireService(
    await requestTool("endpoints_list", {}),
    plan.destination_service_id,
    plan.destination_service_epoch,
  );
  const payload = { jsonrpc: "2.0", id: 1n, method: "tools/list", params: {} };
  const tools = rpcResult(await request(target, payload, null));
  const names = new Set<string>();
  if (Array.isArray(tools.tools))
    for (const item of tools.tools) {
      const parsed = z.record(z.string(), z.unknown()).safeParse(item);
      if (parsed.success && typeof parsed.data.name === "string") names.add(parsed.data.name);
    }
  if (
    !["board_message_post", "board_message_show", "endpoints_list"].every((name) => names.has(name))
  )
    throw new Error("required real MCP tools missing");
  const originStatuses: JsonObject = {};
  for (const origin of [httpsOrigin(plan.https), "https://invalid.example"]) {
    const response = await request(target, payload, { origin });
    if (response.status !== 403) throw new Error("remote Origin was not rejected");
    originStatuses[origin] = response.status;
  }
  const threadRoot = await requestTool("board_message_show", {
    messageId: plan.marker.root_message_id,
  });
  if (
    threadRoot.messageId !== plan.marker.root_message_id ||
    threadRoot.topicId !== plan.marker.topic_id ||
    !sameJsonContent(threadRoot.placement, { kind: "topic", topicId: plan.marker.topic_id }) ||
    !sameJsonContent(threadRoot.actor, plan.actor)
  )
    throw new Error("caller-seeded thread root/topic/actor mismatch");
  return { tools, originStatuses, threadRoot };
}

export async function runPeer(
  plan: BoardPlan,
  request: PeerRequest = httpPost,
  checkAllocation: (deadline: string) => void = requireDeadline,
): Promise<void> {
  checkAllocation(plan.deadline);
  requirePrivateDirectory(plan.evidence);
  const preflight = await peerPreflight(plan, request);
  const postArguments = {
    messageId: plan.marker.message_id,
    placement: { kind: "thread", rootMessageId: plan.marker.root_message_id },
    actor: plan.actor,
    text: plan.marker.text,
    references: [],
  };
  saveRecord(join(plan.evidence, "peer-intent.json"), {
    serviceId: plan.destination_service_id,
    marker: plan.marker,
    tools: preflight.tools,
    originStatuses: preflight.originStatuses,
    threadRoot: preflight.threadRoot,
    postArguments,
  });
  checkAllocation(plan.deadline);
  let response;
  const target: HttpTarget = { kind: "https", endpoint: plan.https };
  try {
    response = await request(target, toolPayload("board_message_post", postArguments), null);
  } catch (error) {
    saveRecord(join(plan.evidence, "peer-post-transport-error.json"), {
      arguments: postArguments,
      error: error instanceof Error ? error.message : String(error),
      errorType: errorCategory(error),
      outcome: "unknown; do not replay",
    });
    throw error;
  }
  saveRecord(join(plan.evidence, "peer-post-response.json"), {
    arguments: postArguments,
    httpStatus: response.status,
    bodyBase64: response.body.toString("base64"),
  });
  const posted = toolResult(response);
  requireMarker(observedObject(posted.message), plan.marker, plan.actor);
  const observed = toolResult(
    await request(
      target,
      toolPayload("board_message_show", { messageId: plan.marker.message_id }),
      null,
    ),
  );
  requireMarker(observed, plan.marker, plan.actor);
  saveRecord(join(plan.evidence, "peer-result.json"), { posted, readback: observed });
}
