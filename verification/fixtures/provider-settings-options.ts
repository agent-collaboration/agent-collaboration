export interface SelectOptionRequest {
  readonly id: string;
  readonly current: string;
  readonly values: readonly string[];
  readonly category?: string;
  readonly label?: string;
}
export function selectOption(input: SelectOptionRequest): Record<string, unknown> {
  return {
    id: input.id,
    name: input.label ?? input.id,
    category: input.category ?? input.id,
    type: "select",
    currentValue: input.current,
    options: input.values.map((value) => ({ value, name: value })),
  };
}
export function modeOption(current: string): Record<string, unknown> {
  return {
    id: "mode",
    name: "Mode",
    category: "mode",
    type: "select",
    currentValue: current,
    options: [
      { value: "auto", name: "Auto" },
      { value: "ask", name: "Ask" },
    ],
  };
}
export const legacyModes = {
  currentModeId: "auto",
  availableModes: [
    { id: "auto", name: "Auto" },
    { id: "ask", name: "Ask" },
  ],
};
