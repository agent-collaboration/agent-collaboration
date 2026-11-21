import { z } from "zod";
import type { WireEntry } from "../../providers/acp/session-control-stdio-gate.ts";

const modeReportSchema = z.looseObject({
  modes: z.looseObject({ currentModeId: z.string() }).optional(),
  currentModeId: z.string().optional(),
  configOptions: z
    .array(
      z.looseObject({
        category: z.string().optional(),
        currentValue: z.unknown(),
      }),
    )
    .optional(),
});

interface CursorRestoreModeEvidence {
  readonly load: number;
  readonly reply: number;
  readonly modeFrame: number;
}

function reportedModes(raw: unknown): string[] {
  const parsed = modeReportSchema.safeParse(raw);
  if (!parsed.success) return [];
  const { modes, currentModeId, configOptions } = parsed.data;
  return [
    modes?.currentModeId,
    currentModeId,
    ...(configOptions
      ?.filter((option) => option.category === "mode")
      .map((option) => option.currentValue) ?? []),
  ].filter((value): value is string => typeof value === "string");
}

export function confirmedCursorAutoAfterLoad(
  wire: readonly WireEntry[],
  sessionId: string,
): CursorRestoreModeEvidence | undefined {
  const forwarded = wire.filter(
    (entry) =>
      entry.sessionId === sessionId && entry.disposition === "forwarded" && entry.frame !== null,
  );
  const loads = forwarded.filter(
    (entry) => entry.direction === "hostToAgent" && entry.method === "session/load",
  );
  const load = loads.length === 1 ? loads[0] : undefined;
  const loadId = load?.frame?.id;
  if (load === undefined || loadId == null) return undefined;
  const reply = forwarded.find(
    (entry) =>
      entry.sequence > load.sequence &&
      entry.direction === "agentToHost" &&
      entry.method === "session/load" &&
      entry.frame?.id === loadId &&
      entry.frame.result !== undefined &&
      entry.frame.error === undefined &&
      entry.forwardedFrame === undefined,
  );
  if (reply === undefined) return undefined;

  const reports = forwarded.flatMap((entry) => {
    if (
      entry.sequence < reply.sequence ||
      entry.direction !== "agentToHost" ||
      entry.forwardedFrame !== undefined ||
      entry.frame?.error !== undefined
    )
      return [];
    const settingReply = forwarded.some(
      (request) =>
        request.sequence > reply.sequence &&
        request.sequence < entry.sequence &&
        request.direction === "hostToAgent" &&
        request.method === "session/set_config_option" &&
        request.frame?.params?.configId === "mode" &&
        request.frame.id != null &&
        request.frame.id === entry.frame?.id,
    );
    const raw =
      entry === reply || (entry.method === "session/set_config_option" && settingReply)
        ? entry.frame?.result
        : entry.method === "session/update"
          ? entry.frame?.params?.update
          : undefined;
    const modes = reportedModes(raw);
    return modes.length === 0 ? [] : [{ sequence: entry.sequence, modes }];
  });
  const latest = reports.at(-1);
  return latest?.modes.every((mode) => mode === "agent") === true
    ? { load: load.sequence, reply: reply.sequence, modeFrame: latest.sequence }
    : undefined;
}
