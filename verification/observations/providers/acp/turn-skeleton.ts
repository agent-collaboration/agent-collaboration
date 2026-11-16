/**
 * O5 turn skeleton: reduces the ordered JSON-RPC frames one Codex CLI exchanged with its
 * app-server (real Codex through the Router relay, or the Router's own face for a Claude
 * Session) to turns, inputs, items and approvals, and checks the Verification Design's
 * structural rules k1–k4 within one run and k5 across the reference and candidate runs.
 *
 * Text and exact sequences are never compared: providers differ in wording, in the number of
 * agent message items and delta chunks, and in whether reasoning or tool items appear.
 *
 * Codex wire shapes are read here with loose Zod schemas because the pinned Codex export is not
 * committed until D1; each schema names only the fields a rule reads.
 */
import { z } from "zod";

export type WireDirection = "cliToServer" | "serverToCli";

/** One JSON-RPC message as the tee saw it, in wire order across both directions. */
export interface WireFrame {
  readonly seq: number;
  /** The tee connection (one per CLI window) the frame travelled on. */
  readonly connection: number;
  readonly direction: WireDirection;
  readonly message: unknown;
}

const requestSchema = z.looseObject({
  id: z.union([z.string(), z.number()]),
  method: z.string(),
  params: z.unknown().optional(),
});
const notificationSchema = z.looseObject({ method: z.string(), params: z.unknown().optional() });
const responseSchema = z.looseObject({
  id: z.union([z.string(), z.number()]),
  result: z.unknown().optional(),
  error: z.unknown().optional(),
});
const inputParamsSchema = z.looseObject({
  clientUserMessageId: z.string().min(1).optional(),
  expectedTurnId: z.string().optional(),
});
const turnParamsSchema = z.looseObject({
  turn: z.looseObject({ id: z.string(), status: z.string().optional() }),
});
const itemSchema = z.looseObject({
  id: z.string(),
  type: z.string(),
  clientId: z.string().optional(),
});
const itemParamsSchema = z.looseObject({ turnId: z.string().optional(), item: itemSchema });
const deltaParamsSchema = z.looseObject({ turnId: z.string().optional(), itemId: z.string() });
const approvalParamsSchema = z.looseObject({ turnId: z.string().optional() });
const tokenUsageParamsSchema = z.looseObject({
  turnId: z.string(),
  tokenUsage: z.looseObject({
    last: z.looseObject({ totalTokens: z.number() }),
    modelContextWindow: z.number().nullable().optional(),
  }),
});
const threadSettingsSchema = z.looseObject({
  model: z.string().nullable().optional(),
  effort: z.string().nullable().optional(),
  serviceTier: z.string().nullable().optional(),
});
const settingsCarrierSchema = z.looseObject({ threadSettings: threadSettingsSchema });

export type InputKind = "start" | "steer";

export interface ScriptInput {
  /** Position among this run's turn/start and turn/steer requests (the k5 correspondence key). */
  readonly ordinal: number;
  readonly kind: InputKind;
  readonly clientId: string | undefined;
  readonly requestSeq: number;
  /** For a steer: the turn the CLI asked to steer. */
  readonly expectedTurnId: string | undefined;
}

export type TurnEntryKind = "itemStarted" | "delta" | "itemCompleted" | "approval" | "tokenUsage";

export interface TurnEntry {
  readonly seq: number;
  readonly kind: TurnEntryKind;
  readonly itemId: string | undefined;
  readonly itemType: string | undefined;
  readonly clientId: string | undefined;
}

export interface SkeletonTurn {
  readonly turnId: string;
  readonly startedSeqs: readonly number[];
  readonly completedSeqs: readonly number[];
  readonly status: string | undefined;
  readonly entries: readonly TurnEntry[];
}

export interface ApprovalExchange {
  readonly method: string;
  readonly turnId: string | undefined;
  readonly promptSeq: number;
  /** The decision the CLI answered with, or undefined if it never answered. */
  readonly decision: string | undefined;
}

export interface ObservedSettings {
  readonly model: string | null;
  readonly effort: string | null;
  readonly serviceTier: string | null;
}

export interface ObservedUsage {
  readonly turnId: string;
  readonly used: number;
  readonly contextWindow: number | null;
}

export interface TurnSkeleton {
  readonly inputs: readonly ScriptInput[];
  readonly turns: readonly SkeletonTurn[];
  readonly approvals: readonly ApprovalExchange[];
  readonly lastSettings: ObservedSettings | undefined;
  readonly usage: readonly ObservedUsage[];
  /** Turn-scoped frames whose turn id never started on this wire. */
  readonly orphanSeqs: readonly number[];
}

interface MutableTurn {
  turnId: string;
  startedSeqs: number[];
  completedSeqs: number[];
  status: string | undefined;
  entries: TurnEntry[];
}

function isDelta(method: string): boolean {
  return method.startsWith("item/") && (method.endsWith("/delta") || method.endsWith("Delta"));
}

/**
 * One settings observation applied to the previous state. An omitted `serviceTier` keeps the
 * previous tier; an explicit `null` clears it (verification.html:139, review F6).
 */
function settingsFrom(
  value: unknown,
  previous: ObservedSettings | undefined,
): ObservedSettings | undefined {
  const parsed = settingsCarrierSchema.safeParse(value);
  if (!parsed.success) return undefined;
  const settings = parsed.data.threadSettings;
  return {
    model: settings.model ?? null,
    effort: settings.effort ?? null,
    serviceTier:
      settings.serviceTier === undefined ? (previous?.serviceTier ?? null) : settings.serviceTier,
  };
}

function decisionFrom(result: unknown): string | undefined {
  const parsed = z.looseObject({ decision: z.unknown() }).safeParse(result);
  if (!parsed.success) return undefined;
  const decision = parsed.data.decision;
  return typeof decision === "string" ? decision : JSON.stringify(decision);
}

const NO_ITEM = { itemId: undefined, itemType: undefined, clientId: undefined } as const;

/** Accumulates one connection's frames, in wire order, into a turn skeleton. */
class SkeletonBuilder {
  private readonly inputs: ScriptInput[] = [];
  private readonly turns = new Map<string, MutableTurn>();
  private readonly turnOrder: string[] = [];
  private readonly approvals: ApprovalExchange[] = [];
  private readonly pendingApprovals = new Map<string, number>();
  private readonly settingsRequests = new Set<string>();
  private readonly usage: ObservedUsage[] = [];
  private readonly orphanSeqs: number[] = [];
  private lastSettings: ObservedSettings | undefined;

  add(frame: WireFrame): void {
    if (frame.direction === "cliToServer") this.fromCli(frame);
    else this.fromServer(frame);
  }

  private turnFor(turnId: string): MutableTurn {
    let turn = this.turns.get(turnId);
    if (turn === undefined) {
      turn = { turnId, startedSeqs: [], completedSeqs: [], status: undefined, entries: [] };
      this.turns.set(turnId, turn);
      this.turnOrder.push(turnId);
    }
    return turn;
  }

  private addEntry(turnId: string | undefined, entry: TurnEntry): void {
    const turn = turnId === undefined ? undefined : this.turns.get(turnId);
    if (turn === undefined) this.orphanSeqs.push(entry.seq);
    else turn.entries.push(entry);
  }

  private fromCli(frame: WireFrame): void {
    const request = requestSchema.safeParse(frame.message);
    if (request.success) {
      const { method, params } = request.data;
      if (method === "turn/start" || method === "turn/steer") {
        const input = inputParamsSchema.safeParse(params ?? {});
        this.inputs.push({
          ordinal: this.inputs.length,
          kind: method === "turn/start" ? "start" : "steer",
          clientId: input.success ? input.data.clientUserMessageId : undefined,
          requestSeq: frame.seq,
          expectedTurnId: input.success ? input.data.expectedTurnId : undefined,
        });
      }
      if (method === "thread/settings/update") this.settingsRequests.add(String(request.data.id));
      return;
    }
    const response = responseSchema.safeParse(frame.message);
    if (!response.success) return;
    const key = String(response.data.id);
    const index = this.pendingApprovals.get(key);
    const prompt = index === undefined ? undefined : this.approvals[index];
    if (index === undefined || prompt === undefined) return;
    this.approvals[index] = { ...prompt, decision: decisionFrom(response.data.result) };
    this.pendingApprovals.delete(key);
  }

  private fromServer(frame: WireFrame): void {
    const request = requestSchema.safeParse(frame.message);
    if (request.success) {
      if (request.data.method.endsWith("/requestApproval"))
        this.approvalPrompt(frame.seq, request.data.id, request.data.method, request.data.params);
      return;
    }
    const response = responseSchema.safeParse(frame.message);
    if (response.success) {
      const settings = settingsFrom(response.data.result, this.lastSettings);
      if (settings !== undefined && this.settingsRequests.has(String(response.data.id)))
        this.lastSettings = settings;
      return;
    }
    const notification = notificationSchema.safeParse(frame.message);
    if (notification.success)
      this.notification(frame.seq, notification.data.method, notification.data.params);
  }

  private approvalPrompt(seq: number, id: string | number, method: string, params: unknown): void {
    const parsed = approvalParamsSchema.safeParse(params ?? {});
    const turnId = parsed.success ? parsed.data.turnId : undefined;
    this.pendingApprovals.set(String(id), this.approvals.length);
    this.approvals.push({ method, turnId, promptSeq: seq, decision: undefined });
    this.addEntry(turnId, { seq, kind: "approval", ...NO_ITEM });
  }

  private notification(seq: number, method: string, params: unknown): void {
    if (method === "turn/started" || method === "turn/completed") {
      const parsed = turnParamsSchema.safeParse(params);
      if (!parsed.success) return;
      const turn = this.turnFor(parsed.data.turn.id);
      if (method === "turn/started") turn.startedSeqs.push(seq);
      else {
        turn.completedSeqs.push(seq);
        turn.status = parsed.data.turn.status;
      }
    } else if (method === "item/started" || method === "item/completed") {
      const parsed = itemParamsSchema.safeParse(params);
      if (!parsed.success) return;
      const { item, turnId } = parsed.data;
      this.addEntry(turnId, {
        seq,
        kind: method === "item/started" ? "itemStarted" : "itemCompleted",
        itemId: item.id,
        itemType: item.type,
        clientId: item.clientId,
      });
    } else if (isDelta(method)) {
      const parsed = deltaParamsSchema.safeParse(params);
      if (!parsed.success) return;
      this.addEntry(parsed.data.turnId, {
        seq,
        kind: "delta",
        ...NO_ITEM,
        itemId: parsed.data.itemId,
      });
    } else if (method === "thread/tokenUsage/updated") {
      const parsed = tokenUsageParamsSchema.safeParse(params);
      if (!parsed.success) return;
      this.usage.push({
        turnId: parsed.data.turnId,
        used: parsed.data.tokenUsage.last.totalTokens,
        contextWindow: parsed.data.tokenUsage.modelContextWindow ?? null,
      });
      this.addEntry(parsed.data.turnId, { seq, kind: "tokenUsage", ...NO_ITEM });
    } else if (method === "thread/settings/updated") {
      this.lastSettings = settingsFrom(params, this.lastSettings) ?? this.lastSettings;
    }
  }

  build(): TurnSkeleton {
    return {
      inputs: this.inputs,
      turns: this.turnOrder.flatMap((turnId) => {
        const turn = this.turns.get(turnId);
        return turn === undefined ? [] : [turn];
      }),
      approvals: this.approvals,
      lastSettings: this.lastSettings,
      usage: this.usage,
      orphanSeqs: this.orphanSeqs,
    };
  }
}

/** Reduces one CLI connection's ordered frames to its turn skeleton; frames from more than one
 * connection are refused, because one window's replay would look like another's duplicate. */
export function reduceTurnSkeleton(frames: readonly WireFrame[]): TurnSkeleton {
  const connections = new Set(frames.map((frame) => frame.connection));
  if (connections.size > 1)
    throw new Error("frames from several CLI connections; reduce each connection separately");
  const builder = new SkeletonBuilder();
  for (const frame of [...frames].sort((left, right) => left.seq - right.seq)) builder.add(frame);
  return builder.build();
}

/** `thread`: the real-Codex reference ran a thread its launch did not open (reference-launch-thread.ts). */
export type SkeletonRule = "k1" | "k2" | "k3" | "k4" | "k5" | "usage" | "thread";

export interface SkeletonViolation {
  readonly rule: SkeletonRule;
  readonly detail: string;
}

const AGENT_ITEM_TYPES: ReadonlySet<string> = new Set(["agentMessage"]);

function isAgentStart(entry: TurnEntry): boolean {
  return (
    entry.kind === "itemStarted" &&
    entry.itemType !== undefined &&
    AGENT_ITEM_TYPES.has(entry.itemType)
  );
}

function userItemsFor(skeleton: TurnSkeleton, clientId: string): TurnEntry[] {
  return skeleton.turns.flatMap((turn) =>
    turn.entries.filter(
      (entry) =>
        entry.kind === "itemStarted" &&
        entry.itemType === "userMessage" &&
        entry.clientId === clientId,
    ),
  );
}

function turnContaining(skeleton: TurnSkeleton, seq: number): SkeletonTurn | undefined {
  return skeleton.turns.find((turn) => turn.entries.some((entry) => entry.seq === seq));
}

/** k1: one start, one completion, nothing of the turn before its start or after its completion. */
function turnLifecycleViolations(turn: SkeletonTurn): SkeletonViolation[] {
  const violations: SkeletonViolation[] = [];
  const k1 = (detail: string): void => {
    violations.push({ rule: "k1", detail: `turn ${turn.turnId}: ${detail}` });
  };
  if (turn.startedSeqs.length !== 1) k1(`started ${turn.startedSeqs.length} times`);
  if (turn.completedSeqs.length !== 1) k1(`completed ${turn.completedSeqs.length} times`);
  const started = turn.startedSeqs[0];
  const completed = turn.completedSeqs[0];
  for (const entry of turn.entries) {
    if (started !== undefined && entry.seq < started)
      k1(`${entry.kind} at ${entry.seq} before its start`);
    if (completed !== undefined && entry.seq > completed)
      k1(`${entry.kind} at ${entry.seq} after its completion`);
  }
  return violations;
}

/**
 * The user message that supersedes an agent message left uncompleted (assignment Correction 58):
 * real Codex 0.166.0 abandons the agent message in flight when a steer arrives (D0 r1 c52: the item
 * started at seq 162 never completes; the steer's user item starts at 1071, a new agent message
 * carries the reply). A userMessage item must start later in the same turn, after the agent
 * message's last own event, and before the turn completes. Every own delta then precedes it.
 */
function supersedingUserStart(turn: SkeletonTurn, lastOwnSeq: number): number | undefined {
  const completed = turn.completedSeqs[0];
  return turn.entries.find(
    (entry) =>
      entry.kind === "itemStarted" &&
      entry.itemType === "userMessage" &&
      entry.seq > lastOwnSeq &&
      (completed === undefined || entry.seq < completed),
  )?.seq;
}

/**
 * k4 for one item: it starts once, completes at most once after its start and its last delta, and
 * its deltas sit inside it. It must complete, except an agent message superseded by a steer.
 */
function itemLifecycleViolations(turn: SkeletonTurn, itemId: string): SkeletonViolation[] {
  const violations: SkeletonViolation[] = [];
  const k4 = (detail: string): void => {
    violations.push({ rule: "k4", detail: `item ${itemId}: ${detail}` });
  };
  const own = turn.entries.filter((entry) => entry.itemId === itemId);
  const starts = own.filter((entry) => entry.kind === "itemStarted");
  const completions = own.filter((entry) => entry.kind === "itemCompleted");
  if (starts.length !== 1) k4(`started ${starts.length} times`);
  const lastOwnSeq = Math.max(...own.map((entry) => entry.seq));
  const superseded =
    completions.length === 0 &&
    starts[0]?.itemType !== undefined &&
    AGENT_ITEM_TYPES.has(starts[0].itemType) &&
    supersedingUserStart(turn, lastOwnSeq) !== undefined;
  if (completions.length > 1 || (completions.length === 0 && !superseded))
    k4(`completed ${completions.length} times`);
  const firstStart = starts[0]?.seq;
  const completion = completions[0]?.seq;
  if (firstStart !== undefined && completion !== undefined && completion < firstStart)
    k4("completed before it started");
  for (const delta of own.filter((entry) => entry.kind === "delta")) {
    if (firstStart === undefined || delta.seq < firstStart)
      k4(`delta ${delta.seq} before the item`);
    if (completion !== undefined && delta.seq > completion) k4(`delta ${delta.seq} after the item`);
  }
  return violations;
}

/** k4 for one turn: every item's lifecycle, and a completed agent message in every answered turn. */
function turnItemViolations(turn: SkeletonTurn): SkeletonViolation[] {
  const itemIds = new Set(
    turn.entries.flatMap((entry) => (entry.itemId === undefined ? [] : [entry.itemId])),
  );
  const violations = [...itemIds].flatMap((itemId) => itemLifecycleViolations(turn, itemId));
  const completedAgent = turn.entries.some(
    (entry) =>
      entry.kind === "itemCompleted" &&
      entry.itemType !== undefined &&
      AGENT_ITEM_TYPES.has(entry.itemType),
  );
  if (turn.status === "completed" && !completedAgent)
    violations.push({
      rule: "k4",
      detail: `answered turn ${turn.turnId} has no completed agent message`,
    });
  return violations;
}

/** k2 and k3 for one input of the action script. */
function inputViolations(skeleton: TurnSkeleton, input: ScriptInput): SkeletonViolation[] {
  const label = `input ${input.ordinal} (${input.kind})`;
  if (input.clientId === undefined)
    return [{ rule: "k2", detail: `${label} sent no client message id` }];
  const userItems = userItemsFor(skeleton, input.clientId);
  const userItem = userItems[0];
  if (userItems.length !== 1 || userItem === undefined)
    return [
      {
        rule: input.kind === "steer" && userItems.length === 0 ? "k3" : "k2",
        detail: `${label} has ${userItems.length} user items with its id`,
      },
    ];
  const turn = turnContaining(skeleton, userItem.seq);
  if (turn === undefined) return [];
  if (input.kind === "start") {
    // k2: the turn-opening input precedes every agent item of its turn.
    const earlierAgent = turn.entries.find(
      (entry) => isAgentStart(entry) && entry.seq < userItem.seq,
    );
    return earlierAgent === undefined
      ? []
      : [
          {
            rule: "k2",
            detail: `turn ${turn.turnId}: agent item at ${earlierAgent.seq} before the opening user item`,
          },
        ];
  }
  // k3: the steered input's receipt lands inside the running turn it steered.
  const violations: SkeletonViolation[] = [];
  if (input.expectedTurnId !== undefined && turn.turnId !== input.expectedTurnId)
    violations.push({
      rule: "k3",
      detail: `${label} landed in turn ${turn.turnId}, not ${input.expectedTurnId}`,
    });
  const completed = turn.completedSeqs[0];
  if (completed === undefined || userItem.seq > completed)
    violations.push({ rule: "k3", detail: `${label} receipt is not inside its running turn` });
  return violations;
}

/** k1–k4 within one run. */
export function checkRunRules(skeleton: TurnSkeleton): SkeletonViolation[] {
  return [
    ...skeleton.orphanSeqs.map((seq): SkeletonViolation => ({
      rule: "k1",
      detail: `frame ${seq} names a turn that never started`,
    })),
    ...skeleton.turns.flatMap(turnLifecycleViolations),
    ...skeleton.turns.flatMap(turnItemViolations),
    ...skeleton.inputs.flatMap((input) => inputViolations(skeleton, input)),
  ];
}

/** The provider's own report for one turn, read from the Router session record's Usage item. */
export interface ProviderUsage {
  readonly turnId: string;
  readonly used: number;
  readonly size: number;
}

/** Usage: the CLI must receive the provider's own used and size for that turn. */
export function checkProviderUsage(
  skeleton: TurnSkeleton,
  provider: readonly ProviderUsage[],
): SkeletonViolation[] {
  const violations: SkeletonViolation[] = [];
  if (provider.length === 0)
    violations.push({
      rule: "usage",
      detail: "no provider usage report was observed for this run",
    });
  for (const reported of provider) {
    const received = skeleton.usage.filter((entry) => entry.turnId === reported.turnId).at(-1);
    if (received === undefined) {
      violations.push({
        rule: "usage",
        detail: `turn ${reported.turnId}: provider reported usage the CLI never received`,
      });
      continue;
    }
    if (received.contextWindow !== reported.size)
      violations.push({
        rule: "usage",
        detail: `turn ${reported.turnId}: CLI window ${received.contextWindow}, provider size ${reported.size}`,
      });
    if (received.used !== reported.used)
      violations.push({
        rule: "usage",
        detail: `turn ${reported.turnId}: CLI used ${received.used}, provider used ${reported.used}`,
      });
  }
  return violations;
}

/** What the action script picked for one target after `/model`. */
export interface ScriptedSettings {
  readonly model: string;
  readonly effort: string | null;
  readonly serviceTier: string | null;
}

export interface RunUnderComparison {
  readonly skeleton: TurnSkeleton;
  /** Present when the script ran `/model` on this target. */
  readonly scriptedSettings: ScriptedSettings | undefined;
  /** The decision that reached the agent per approval, in prompt order, when observable. */
  readonly agentApprovalOutcomes: readonly string[] | undefined;
}

/** k5: correspondence between the reference run and the candidate run. */
export function checkCrossRun(
  reference: RunUnderComparison,
  candidate: RunUnderComparison,
): SkeletonViolation[] {
  const violations: SkeletonViolation[] = [];
  const left = reference.skeleton;
  const right = candidate.skeleton;
  if (left.turns.length !== right.turns.length)
    violations.push({
      rule: "k5",
      detail: `turn count ${left.turns.length} vs ${right.turns.length}`,
    });
  left.turns.forEach((turn, index) => {
    const other = right.turns[index];
    if (other !== undefined && other.status !== turn.status)
      violations.push({
        rule: "k5",
        detail: `turn ${index} status ${turn.status} vs ${other.status}`,
      });
  });
  if (left.inputs.length !== right.inputs.length)
    violations.push({
      rule: "k5",
      detail: `input count ${left.inputs.length} vs ${right.inputs.length}`,
    });
  left.inputs.forEach((input, index) => {
    const other = right.inputs[index];
    if (other !== undefined && other.kind !== input.kind)
      violations.push({ rule: "k5", detail: `input ${index} ${input.kind} vs ${other.kind}` });
  });
  if (left.approvals.length !== right.approvals.length)
    violations.push({
      rule: "k5",
      detail: `approval prompts ${left.approvals.length} vs ${right.approvals.length}`,
    });
  left.approvals.forEach((approval, index) => {
    const other = right.approvals[index];
    if (other !== undefined && other.decision !== approval.decision)
      violations.push({
        rule: "k5",
        detail: `approval ${index} CLI decision ${approval.decision} vs ${other.decision}`,
      });
  });
  for (const [label, run] of [
    ["reference", reference],
    ["candidate", candidate],
  ] as const) {
    const outcomes = run.agentApprovalOutcomes;
    if (outcomes === undefined && run.skeleton.approvals.length > 0)
      violations.push({
        rule: "k5",
        detail: `${label}: approval outcome at the agent was not observed`,
      });
    if (outcomes !== undefined) {
      run.skeleton.approvals.forEach((approval, index) => {
        if (outcomes[index] !== approval.decision)
          violations.push({
            rule: "k5",
            detail: `${label} approval ${index}: CLI chose ${approval.decision}, agent got ${outcomes[index]}`,
          });
      });
    }
    const scripted = run.scriptedSettings;
    if (scripted !== undefined) {
      const observed = run.skeleton.lastSettings;
      if (
        observed === undefined ||
        observed.model !== scripted.model ||
        observed.effort !== scripted.effort ||
        observed.serviceTier !== scripted.serviceTier
      )
        violations.push({
          rule: "k5",
          detail: `${label} settings ${JSON.stringify(observed)} after /model, script picked ${JSON.stringify(scripted)}`,
        });
    }
  }
  return violations;
}
