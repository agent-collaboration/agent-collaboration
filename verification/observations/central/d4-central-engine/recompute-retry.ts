/** Tokio's quiet one-shot exact-write recovery; the caller owns fixture shutdown. */
import type { D4ControlClient } from "./control-client.ts";
import type { D4EventRecord } from "./event-observation.ts";

type RecomputeRetryCheck =
  | "quiet-fixture"
  | "passive-fault-control"
  | "one-shot-fault-reached"
  | "failed-alarm-requests-retry"
  | "platform-retry-deadline"
  | "one-automatic-retry"
  | "one-shot-fault-consumed"
  | "quiet-after-recovery";

export class RecomputeRetryFailure extends Error {
  readonly check: RecomputeRetryCheck;
  readonly actual: unknown;
  readonly expected: unknown;
  constructor(check: RecomputeRetryCheck, actual: unknown, expected: unknown) {
    super(`J18 recompute-retry assertion failed: ${check}`);
    this.check = check;
    this.actual = actual;
    this.expected = expected;
  }
}

function requireQuietPending(pending: { tasks: bigint; deadlines: bigint; waiters: bigint }): void {
  if (Object.values(pending).some((count): boolean => count !== 0n))
    throw new RecomputeRetryFailure("quiet-fixture", pending, "no active event work");
}

function onlyAlarms(records: readonly D4EventRecord[]): void {
  if (records.some((record): boolean => record.event.kind !== "alarm"))
    throw new RecomputeRetryFailure("one-automatic-retry", records, "only the selected Alarm pair");
}

export async function driveTokioRecomputeRetry(input: { readonly controls: D4ControlClient }) {
  const { controls } = input;
  requireQuietPending(await controls.readPending());
  const initialAlarm = await controls.readAlarm();
  if (initialAlarm !== null) throw new RecomputeRetryFailure("quiet-fixture", initialAlarm, null);
  const baseline = await controls.readEvents(0n);
  const faultId = await controls.armFault({ kind: "exactAlarmWrite", value: { on: "alarm" } }, 1n);
  const afterArm = await controls.readEvents(baseline.through);
  if (afterArm.records.length !== 0)
    throw new RecomputeRetryFailure("passive-fault-control", afterArm.records, "no kernel event");
  // Exactly one deliberate stimulus. The second Alarm must come from the
  // platform-owned retry when the qualified clock epoch is drained.
  await controls.duplicateAlarm();
  const firstPage = await controls.readEvents(baseline.through);
  const firstRecords = firstPage.records.map((item): D4EventRecord => item.record);
  onlyAlarms(firstRecords);
  const failed = firstRecords[0];
  const consumed = await controls.readFault(faultId);
  if (consumed.armed !== 1n || consumed.consumed !== 1n || consumed.reached !== 1n)
    throw new RecomputeRetryFailure("one-shot-fault-reached", consumed, {
      armed: 1n,
      reached: 1n,
      consumed: 1n,
    });
  if (firstRecords.length !== 1 || failed?.reply.kind !== "alarmRetry")
    throw new RecomputeRetryFailure("failed-alarm-requests-retry", firstRecords, "one AlarmRetry");
  const deadline = await controls.readAlarm();
  if (deadline === null || deadline <= failed.enteredAt)
    throw new RecomputeRetryFailure("platform-retry-deadline", deadline, "future owned retry");
  const settled = await controls.settleClock(new Date(Number(deadline + 1n)).toISOString());
  const finalPage = await controls.readEvents(baseline.through);
  const records = finalPage.records.map((item): D4EventRecord => item.record);
  onlyAlarms(records);
  const recovered = records[1];
  if (
    settled !== 1n ||
    records.length !== 2 ||
    records[0]?.eventId !== failed.eventId ||
    recovered?.eventId === failed.eventId ||
    recovered?.reply.kind !== "alarmDone" ||
    recovered.enteredAt <= deadline
  )
    throw new RecomputeRetryFailure(
      "one-automatic-retry",
      { settled, records },
      "one later AlarmDone",
    );
  const fault = await controls.readFault(faultId);
  if (fault.armed !== 1n || fault.consumed !== 1n || fault.reached !== 2n)
    throw new RecomputeRetryFailure("one-shot-fault-consumed", fault, {
      armed: 1n,
      reached: 2n,
      consumed: 1n,
    });
  const stored = await controls.readAlarm();
  if (stored !== null) throw new RecomputeRetryFailure("quiet-after-recovery", stored, null);
  requireQuietPending(await controls.readPending());
  return {
    journey: "J18" as const,
    shell: "tokio" as const,
    subcase: "quiet-exact-write-recovery" as const,
    faultId,
    fault,
    retryDeadline: deadline,
    records,
    storedAfter: stored,
    runtimeQualified: false as const,
    remainingQualification: [
      "declared signed artifact, complete original S1 and qualified causal clock drain",
      "fresh drained owning sources, pinned business clock and enabled automatic alarms",
      "fault cuts the real end-of-Alarm exact setter before its write",
      "wrong AlarmDone-on-failure artifact fails the same case and original passes again",
      "other J18 interleavings and nonempty work/effect recovery remain separate cases",
    ],
  };
}
