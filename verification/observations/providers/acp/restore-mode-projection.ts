// Verification-only projection. Preserve the real load reply except for the
// mode in the native and config-option representations the agent supplied.
import { z } from "zod";

export const loadModeProjectionSchema = z.strictObject({
  sessionId: z.string().min(1),
  mode: z.enum(["default", "acceptEdits", "ask"]),
});

const loadModeFields = z.looseObject({
  modes: z
    .looseObject({
      currentModeId: z.string(),
      availableModes: z.array(z.looseObject({ id: z.string() })),
    })
    .nullable()
    .optional(),
  configOptions: z
    .array(
      z.looseObject({
        category: z.string().optional(),
        currentValue: z.unknown().optional(),
        options: z.unknown().optional(),
      }),
    )
    .nullable()
    .optional(),
});

export function projectLoadMode(
  result: unknown,
  mode: string,
): { result: unknown; changed: boolean } {
  const loaded = loadModeFields.parse(result);
  let changed = false;
  let offered = false;
  const modes = loaded.modes;
  if (modes !== undefined && modes !== null) {
    if (!modes.availableModes.some((option) => option.id === mode))
      throw new Error("the projected legacy mode is not offered");
    offered = true;
    changed = modes.currentModeId !== mode;
    loaded.modes = { ...modes, currentModeId: mode };
  }
  if (loaded.configOptions !== undefined && loaded.configOptions !== null) {
    loaded.configOptions = loaded.configOptions.map((option) => {
      if (option.category !== "mode") return option;
      const choices = z.array(z.looseObject({ value: z.string() })).parse(option.options);
      if (
        !choices.some((choice) => choice.value === mode) ||
        typeof option.currentValue !== "string"
      )
        throw new Error("the projected config mode is not an offered selection");
      offered = true;
      changed ||= option.currentValue !== mode;
      return { ...option, currentValue: mode };
    });
  }
  if (!offered) throw new Error("the load reply has no advertised mode to project");
  return { result: loaded, changed };
}
