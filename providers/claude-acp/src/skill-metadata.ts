import * as z from "zod/mini";

const descriptorSchema = z.looseObject({
  name: z.string().check(z.minLength(1)),
  description: z.string(),
  argumentHint: z.string(),
  aliases: z.optional(z.array(z.string())),
  builtin: z.optional(z.boolean()),
});
const descriptorsSchema = z.array(descriptorSchema);
const reloadSchema = z.object({ skills: descriptorsSchema });
const commandSchema = z.looseObject({
  name: z.string(),
  description: z.string(),
  _meta: z.optional(z.record(z.string(), z.unknown())),
});
export const notificationSchema = z.looseObject({
  sessionId: z.string(),
  update: z.looseObject({
    sessionUpdate: z.string(),
    availableCommands: z.optional(z.array(commandSchema)),
    _meta: z.optional(z.record(z.string(), z.unknown())),
  }),
});
export type MetadataNotification = z.infer<typeof notificationSchema>;
type CommandDescriptor = z.infer<typeof descriptorSchema>;

export interface MetadataQuery {
  readonly sessionIdentity: object;
  readonly queryIdentity: object;
  supportedCommands(): Promise<unknown>;
  reloadSkills(): Promise<unknown>;
}
interface MetadataInputs {
  readonly currentQuery: (sessionId: string) => MetadataQuery | undefined;
  readonly send: (notification: MetadataNotification) => Promise<void>;
}
type SkillRead = { kind: "available"; skills: CommandDescriptor[] } | { kind: "unavailable" };
interface MetadataState {
  readonly query: MetadataQuery;
  latest: MetadataNotification;
  dirty: boolean;
  task?: Promise<void>;
  namespace?: string;
  result?: SkillRead;
  failedInput?: string;
}

function descriptorKey(descriptors: CommandDescriptor[]): string {
  return JSON.stringify(
    descriptors.map((row) => [
      row.name,
      row.description,
      row.argumentHint,
      row.aliases ?? [],
      row.builtin ?? false,
    ]),
  );
}

function advertisedKey(notification: MetadataNotification): string {
  return JSON.stringify(
    (notification.update.availableCommands ?? []).map((row) => [
      row.name,
      row.description,
      row.input ?? null,
    ]),
  );
}

function belongsToSkill(
  name: string,
  descriptors: CommandDescriptor[],
  skills: CommandDescriptor[],
): boolean {
  const exact = descriptors.filter((row) => row.name === name);
  const builtin = exact.filter((row) => row.builtin === true);
  const candidates =
    exact.length > 0
      ? builtin.length > 0
        ? builtin
        : exact
      : descriptors.filter((row) => row.aliases?.includes(name));
  const selected = candidates.length === 1 ? candidates[0] : undefined;
  return (
    selected !== undefined &&
    skills.some(
      (skill) =>
        skill.name === selected.name && (skill.builtin ?? false) === (selected.builtin ?? false),
    )
  );
}

function annotate(
  latest: MetadataNotification,
  descriptors: CommandDescriptor[],
  result: SkillRead,
): MetadataNotification {
  const rows = latest.update.availableCommands ?? [];
  const counts = new Map<string, number>();
  for (const row of rows) counts.set(row.name, (counts.get(row.name) ?? 0) + 1);
  return {
    ...latest,
    update: {
      ...latest.update,
      availableCommands: rows.map((row) =>
        result.kind === "available" &&
        counts.get(row.name) === 1 &&
        belongsToSkill(row.name, descriptors, result.skills)
          ? { ...row, _meta: { ...row._meta, codexRouter: { kind: "skill" } } }
          : row,
      ),
      _meta: { ...latest.update._meta, codexRouter: { skillStatus: result.kind } },
    },
  };
}

/** Per-query metadata derivation. The public Query remains the source and executor. */
export class SkillMetadataDecorator {
  private readonly inputs: MetadataInputs;
  private readonly sessions = new Map<string, MetadataState>();

  constructor(inputs: MetadataInputs) {
    this.inputs = inputs;
  }

  async sessionUpdate(notification: MetadataNotification): Promise<void> {
    await this.inputs.send(notification);
    if (notification.update.sessionUpdate !== "available_commands_update") return;
    const query = this.inputs.currentQuery(notification.sessionId);
    if (!query) {
      await this.inputs.send(annotate(notification, [], { kind: "unavailable" }));
      return;
    }
    let state = this.sessions.get(notification.sessionId);
    if (
      !state ||
      state.query.queryIdentity !== query.queryIdentity ||
      state.query.sessionIdentity !== query.sessionIdentity
    ) {
      state = { query, latest: notification, dirty: true };
      this.sessions.set(notification.sessionId, state);
    } else {
      state.latest = notification;
      state.dirty = true;
    }
    this.schedule(notification.sessionId, state);
  }

  private schedule(sessionId: string, state: MetadataState): void {
    if (state.task || !state.dirty || !this.current(sessionId, state)) return;
    state.task = this.refresh(sessionId, state).finally(() => {
      state.task = undefined;
      this.schedule(sessionId, state);
    });
  }

  private current(sessionId: string, state: MetadataState): boolean {
    const query = this.inputs.currentQuery(sessionId);
    return (
      this.sessions.get(sessionId) === state &&
      query?.sessionIdentity === state.query.sessionIdentity &&
      query.queryIdentity === state.query.queryIdentity
    );
  }

  private async refresh(sessionId: string, state: MetadataState): Promise<void> {
    while (state.dirty && this.current(sessionId, state)) {
      state.dirty = false;
      const input = advertisedKey(state.latest);
      if (state.failedInput === input) {
        await this.inputs.send(annotate(state.latest, [], { kind: "unavailable" }));
        continue;
      }
      state.failedInput = undefined;
      try {
        const descriptors = descriptorsSchema.parse(await state.query.supportedCommands());
        if (!this.current(sessionId, state)) return;
        const key = JSON.stringify([input, descriptorKey(descriptors)]);
        if (state.namespace !== key) {
          state.namespace = key;
          state.result = undefined;
          try {
            const reloaded = reloadSchema.parse(await state.query.reloadSkills());
            state.result = { kind: "available", skills: reloaded.skills };
          } catch {
            state.result = { kind: "unavailable" };
          }
        }
        if (!this.current(sessionId, state)) return;
        await this.inputs.send(
          annotate(state.latest, descriptors, state.result ?? { kind: "unavailable" }),
        );
      } catch {
        if (!this.current(sessionId, state)) return;
        state.failedInput = input;
        await this.inputs.send(annotate(state.latest, [], { kind: "unavailable" }));
      }
    }
  }

  async settled(): Promise<void> {
    for (;;) {
      const tasks = [...this.sessions.values()].flatMap((state) =>
        state.task ? [state.task] : [],
      );
      if (tasks.length === 0) return;
      await Promise.all(tasks);
    }
  }
}
