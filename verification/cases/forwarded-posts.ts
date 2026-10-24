import { z } from "zod";
import {
  acceptedWrite,
  boardIdentity,
  feedCursorSchema,
  writeHeader,
  type FeedCursor,
} from "../clients/board-requests.ts";
import type { CaseRegistration } from "../coverage.ts";
import { field } from "../exact-json.ts";
import { ObservationOpen, type Receipt } from "../receipt.ts";
import type { PreparedRun } from "../run-setup.ts";
import { freezeNotification, finishNotification } from "./notifications.ts";

export const forwardedPostsCase = {
  id: "forwarded-post-origin-replay-and-offline-holder",
  rows: ["OB4:F"],
  evidence: "journey",
} satisfies CaseRegistration;
const viewSchema = z.object({ asOf: feedCursorSchema });
const feedSchema = z.object({
  value: z.object({
    kind: z.literal("entries"),
    entries: z.array(
      z.object({
        record: z.object({ kind: z.string(), id: z.string() }),
        origin: z.object({ installation: z.string(), machine: z.string() }),
        actor: z.unknown(),
        request: z.string().nullable().optional(),
      }),
    ),
    coveredThrough: feedCursorSchema,
    highWater: feedCursorSchema,
  }),
});
async function observeOrigins(input: {
  context: PreparedRun;
  receipt: Receipt;
  after: FeedCursor;
  posts: readonly { readonly postId: string; readonly requestId: string }[];
}): Promise<void> {
  const [holder, requester] = input.context.machines;
  const feed = feedSchema.parse(
    await holder.installation.mcp.call("feed_read", {
      projectId: holder.heldProject,
      reader: { kind: "anonymous" },
      after: input.after,
      limit: 100n,
      includeRedacted: false,
    }),
  ).value;
  await input.receipt.equal(
    "forwarding observation covers its complete feed page",
    feed.coveredThrough,
    feed.highWater,
  );
  const origin = await requester.installation.identity();
  for (const post of input.posts) {
    const entries = feed.entries.filter(
      (entry): boolean => entry.record.kind === "post" && entry.record.id === post.postId,
    );
    await input.receipt.equal(
      "forwarded post has one verified sender origin",
      entries.map((entry) => ({
        origin: entry.origin,
        actor: entry.actor,
        request: entry.request,
      })),
      [
        {
          origin: {
            installation: origin.serviceId,
            machine: requester.installation.enrollmentLabel,
          },
          actor: requester.actor,
          request: post.requestId,
        },
      ],
    );
  }
}
async function onlineForwarding(
  context: PreparedRun,
  receipt: Receipt,
): Promise<z.infer<typeof viewSchema>> {
  const [holder, requester] = context.machines;
  const projectId = holder.heldProject;
  const before = viewSchema.parse(
    await holder.installation.mcp.call("view_work", {
      projectId,
      includeRemoved: false,
    }),
  );
  const notification = {
    source: requester.installation,
    actor: requester.actor,
    project: projectId,
    recipients: [requester],
    receipt,
  };
  const root = await freezeNotification(notification);
  const reply = {
    ...writeHeader(requester.actor),
    requestId: boardIdentity(),
    postId: boardIdentity(),
    placement: { kind: "thread", thread: { project: projectId, thread: root.post } },
    text: `verification-forwarded-reply-${boardIdentity()}`,
    references: [],
    addressees: [],
  };
  const decision = await acceptedWrite({
    client: requester.installation.mcp,
    tool: "thread_post",
    request: reply,
  });
  for (const post of [
    { postId: root.post, requestId: root.request.requestId, text: root.request.text },
    reply,
  ]) {
    const shown = await holder.installation.mcp.call("post_show", { postId: post.postId });
    await receipt.equal(
      "online holder stores the literal forwarded post exactly",
      z
        .object({ postId: z.string(), projectId: z.string(), actor: z.unknown(), text: z.string() })
        .parse(shown),
      { postId: post.postId, projectId, actor: requester.actor, text: post.text },
    );
  }
  await finishNotification(notification, root);
  const completed = viewSchema.parse(
    await holder.installation.mcp.call("view_work", {
      projectId,
      includeRemoved: false,
    }),
  );
  await receipt.equal(
    "forwarded reply replays the holder's exact decision",
    await acceptedWrite({
      client: requester.installation.mcp,
      tool: "thread_post",
      request: reply,
    }),
    decision,
  );
  await receipt.equal(
    "forwarded replay adds no holder feed effect",
    viewSchema.parse(
      await holder.installation.mcp.call("view_work", { projectId, includeRemoved: false }),
    ),
    completed,
  );
  await observeOrigins({
    context,
    receipt,
    after: before.asOf,
    posts: [{ postId: root.post, requestId: z.string().parse(root.request.requestId) }, reply],
  });
  return completed;
}
async function publicRefusals(input: {
  context: PreparedRun;
  receipt: Receipt;
  completed: z.infer<typeof viewSchema>;
}): Promise<void> {
  const { context, receipt, completed } = input;
  const [holder, requester] = context.machines;
  const projectId = holder.heldProject;
  const gate = context.gates.get(holder.installation.machine.name);
  if (gate === undefined) throw new ObservationOpen("holder Session gate is unavailable");
  const unheld = context.imported.report.projects[0];
  if (unheld === undefined)
    throw new ObservationOpen("no actual imported unheld project is available");
  const root = {
    ...writeHeader(requester.actor),
    postId: boardIdentity(),
    projectId: unheld.project,
    about: { kind: "project", subject: { line: "Unheld forwarding refusal" } },
    text: "Disposable forwarding refusal",
    references: [],
    addressees: [],
    role: null,
    watch: false,
  };
  await receipt.equal(
    "an actually imported unheld project refuses forwarding",
    field(await requester.installation.mcp.call("thread_create", root), "result", "kind"),
    "notHeld",
  );
  const nonPost = await requester.installation.mcp.call("workstream_create", {
    ...writeHeader(requester.actor),
    projectId,
    operation: {
      kind: "workstreamCreate",
      workstreamId: boardIdentity(),
      name: "Refused forwarding",
      goal: "Must remain at the holder",
    },
  });
  await receipt.equal(
    "a non-post write at the nonholder names the known writer epoch",
    z
      .object({
        kind: z.literal("notWriter"),
        decision: z.object({ current: z.bigint().nullable() }),
      })
      .parse(field(nonPost, "result")),
    { kind: "notWriter", decision: { current: completed.asOf.epoch } },
  );
  const connected = await gate.ready();
  gate.pause();
  try {
    const refused = await requester.installation.mcp.call("thread_create", {
      ...root,
      projectId,
      ...writeHeader(requester.actor),
      postId: boardIdentity(),
    });
    await receipt.equal(
      "offline holder returns its typed forwarding refusal",
      field(refused, "result", "kind"),
      "holderUnreachable",
    );
  } finally {
    gate.resume();
  }
  await gate.ready(connected.welcomes);
  await receipt.equal(
    "offline forwarding writes no holder effect",
    viewSchema.parse(
      await holder.installation.mcp.call("view_work", { projectId, includeRemoved: false }),
    ),
    completed,
  );
}
export async function forwardedPosts(context: PreparedRun, receipt: Receipt): Promise<void> {
  const completed = await onlineForwarding(context, receipt);
  await publicRefusals({ context, receipt, completed });
  await receipt.record("forwardingProofAllocation", {
    deliveryHalfObserved: true,
    nonRetentionEvidenceOwner:
      "B F2 (T8): native and Worker owning-state tests with retainer control",
    nonRetentionObserved: false,
  });
}
