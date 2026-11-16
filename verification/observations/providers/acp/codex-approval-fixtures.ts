/**
 * Recorded two-window approval wires for the O5 approval comparison (a1-a3). A is connection 0
 * (the approving window), B connection 1 (the watching window). Fixture data shaped like Codex's
 * app-server frames, never a live recording.
 */
import type { WireFrame } from "./turn-skeleton.ts";

export interface ApprovalPairOptions {
  readonly method?: string;
  readonly choices?: readonly string[] | null;
  /** The CLI's decision; null when the prompt times out with no answer. */
  readonly decision?: string | null;
  /** The status the approval's item completes with on A's wire (real Codex's own outcome). */
  readonly itemStatus?: string;
  /** No server request: only an item that says approval is pending. */
  readonly textOnly?: boolean;
  readonly bPrompted?: boolean;
  /** B never sees the approval item's result. */
  readonly bMissesResult?: boolean;
  /** B sees the approval item's result with another status than A. */
  readonly bResultStatus?: string;
  /** B sees the approval item in another turn. */
  readonly bTurnId?: string;
  /**
   * B sees an unrelated agent message start before the answer and complete after it, and never
   * the approval item itself (review F5's substitution counterexample).
   */
  readonly bSubstitutes?: boolean;
  readonly noWindowB?: boolean;
}

/** A command approval as two windows saw it: A is connection 0, B connection 1. */
export function approvalPairRun(options: ApprovalPairOptions = {}): readonly WireFrame[] {
  const frames: WireFrame[] = [];
  const push = (connection: number, direction: WireFrame["direction"], message: unknown): void => {
    frames.push({ seq: frames.length, connection, direction, message });
  };
  const turn = (connection: number, phase: string, status: string): void =>
    push(connection, "serverToCli", {
      method: `turn/${phase}`,
      params: { threadId: "t", turn: { id: "T1", status } },
    });
  const item = (
    connection: number,
    phase: string,
    id: string,
    type: string,
    status: string,
    turnId = "T1",
  ): void =>
    push(connection, "serverToCli", {
      method: `item/${phase}`,
      params: { threadId: "t", turnId, item: { id, type, status } },
    });
  const request = (connection: number): void =>
    push(connection, "serverToCli", {
      ["id"]: "req-1",
      method: options.method ?? "item/commandExecution/requestApproval",
      params: {
        itemId: "item-1",
        turnId: "T1",
        ...(options.choices === null
          ? {}
          : {
              availableDecisions: options.choices ?? [
                "accept",
                "acceptForSession",
                "decline",
                "cancel",
              ],
            }),
      },
    });
  const windowB = options.noWindowB !== true;
  const resultStatus = options.itemStatus ?? "completed";
  turn(0, "started", "inProgress");
  if (windowB) turn(1, "started", "inProgress");
  if (options.textOnly === true) item(0, "started", "pending-text", "agentMessage", "inProgress");
  else {
    item(0, "started", "item-1", "commandExecution", "inProgress");
    request(0);
  }
  if (options.bPrompted === true) request(1);
  if (windowB) {
    if (options.bSubstitutes === true) item(1, "started", "other-1", "agentMessage", "inProgress");
    else item(1, "started", "item-1", "commandExecution", "inProgress", options.bTurnId);
  }
  const decision = options.decision === undefined ? "accept" : options.decision;
  if (decision !== null) push(0, "cliToServer", { ["id"]: "req-1", result: { decision } });
  item(0, "completed", "item-1", "commandExecution", resultStatus);
  if (windowB && options.bMissesResult !== true) {
    if (options.bSubstitutes === true) item(1, "completed", "other-1", "agentMessage", "completed");
    else
      item(
        1,
        "completed",
        "item-1",
        "commandExecution",
        options.bResultStatus ?? resultStatus,
        options.bTurnId,
      );
  }
  turn(0, "completed", "completed");
  if (windowB) turn(1, "completed", "completed");
  return frames;
}
