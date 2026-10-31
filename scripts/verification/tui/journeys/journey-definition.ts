import { z } from 'zod';
export const journeyKeySchema = z.enum([
	'Ctrl+D',
	'Ctrl+E',
	'Ctrl+F',
	'Ctrl+G',
	'Ctrl+N',
	'Ctrl+R',
	'Down',
	'Enter',
	'Escape',
	'F1',
	'F4',
	'F5',
	'PageDown',
	'PageUp',
	'Right',
	'Shift+Tab',
	'Tab',
	'Up',
]);
export const journeyActionSchema = z.discriminatedUnion('kind', [
	z.object({
		kind: z.literal('key'),
		key: journeyKeySchema,
		repeat: z.number().int().min(1).max(256),
	}),
	z.object({ kind: z.literal('text'), text: z.string() }),
	z.object({ kind: z.literal('wait'), pattern: z.string() }),
	z.object({ kind: z.literal('field'), dialog: z.string(), label: z.string(), value: z.string() }),
]);
export const journeyDefinitionSchema = z.object({
	name: z.string(),
	groups: z.array(z.object({ checkpoint: z.string(), actions: z.array(journeyActionSchema) })),
	finish: z.array(journeyActionSchema),
});
export type JourneyAction = z.infer<typeof journeyActionSchema>;
export type JourneyDefinition = z.infer<typeof journeyDefinitionSchema>;

/** The owner-adopted viewport matrix, shared by screen, flow and input journeys. */
export const journeyGridSizes = ['120x36', '76x30', '48x12', '32x24'] as const;
export type JourneyGridSize = (typeof journeyGridSizes)[number];

interface ScreenJourneyGroup {
	readonly checkpoint: string;
	readonly actions: readonly JourneyAction[];
	readonly grids?: readonly JourneyGridSize[];
	readonly actionsByGrid?: Readonly<Partial<Record<JourneyGridSize, readonly JourneyAction[]>>>;
}
export interface ScreenJourney {
	readonly screen: 'sessions' | 'normal' | 'hard' | 'quota';
	readonly groups: readonly ScreenJourneyGroup[];
	readonly finish: readonly JourneyAction[];
}

export function expandScreenJourney(
	screen: ScreenJourney,
	grid: JourneyGridSize,
): JourneyDefinition {
	return {
		name: screen.screen + '-' + grid,
		groups: screen.groups
			.filter((group) => group.grids === undefined || group.grids.includes(grid))
			.map((group) => ({
				checkpoint: group.checkpoint,
				actions: [...(group.actionsByGrid?.[grid] ?? group.actions)],
			})),
		finish: [...screen.finish],
	};
}
export function keyPress(key: z.infer<typeof journeyKeySchema>, repeat = 1): JourneyAction {
	return { kind: 'key', key, repeat };
}
export function typedText(text: string): JourneyAction {
	return { kind: 'text', text };
}
export function visibleText(pattern: string): JourneyAction {
	return { kind: 'wait', pattern };
}
export function dialogField(dialog: string, label: string, value: string): JourneyAction {
	return { kind: 'field', dialog, label, value };
}
