/** Showcase receipts are private wire projections, never domain or dispatcher state. */
import { z } from 'zod';

import { sourceBindingSchema } from '../evidence-schema.ts';
export const scenarioIds = [
	'SC01',
	'SC02',
	'SC03',
	'SC04',
	'SC05',
	'SC06',
	'SC07',
	'SC08',
	'SC09',
	'SC10',
	'SC11',
	'SC12',
	'SC13',
	'SC14',
	'SC15',
	'SC16',
	'SC17',
	'SC18',
	'SC19',
	'SC20',
	'SC21',
	'SC22',
	'SC23',
	'SC24',
	'SC25',
	'SC26',
	'SC27',
] as const;
export type ScenarioId = (typeof scenarioIds)[number];
export const adoptedGrids = [
	{ columns: 120, rows: 36 },
	{ columns: 76, rows: 30 },
	{ columns: 48, rows: 24 },
	{ columns: 48, rows: 12 },
	{ columns: 40, rows: 24 },
	{ columns: 32, rows: 24 },
] as const;
const artifact = z.strictObject({ path: z.string(), sha256: z.string().regex(/^[a-f0-9]{64}$/u) });
export const showcasePlanSchema = z.strictObject({
	kind: z.literal('component-showcase'),
	repository: z.string(),
	source: sourceBindingSchema,
	driver: artifact,
	example: artifact,
	node: artifact,
	output: z.string(),
	chrome: z.string().optional(),
	keys: z
		.array(z.enum(['legacy', 'kitty']))
		.min(1)
		.default(['legacy', 'kitty']),
	grids: z
		.array(
			z.strictObject({
				columns: z.number().int().positive().max(4096),
				rows: z.number().int().positive().max(4096),
			}),
		)
		.min(1)
		.default([...adoptedGrids]),
	cases: z
		.array(z.enum(scenarioIds))
		.min(1)
		.default([...scenarioIds]),
	variantLabels: z.array(z.string().min(1)).min(1).optional(),
	inputProfile: z.enum(['terminal-only', 'example-ghostty-keybinds']).default('terminal-only'),
});
export type ShowcasePlan = z.infer<typeof showcasePlanSchema>;
const surface = z.string().regex(/^(?:screen|overlay:[1-9][0-9]*)$/u);
const cell = z.tuple([
	z.number().int().nonnegative().max(65535),
	z.number().int().nonnegative().max(65535),
]);
const action = z.strictObject({
	actionId: z.string().min(1),
	title: z.string(),
	toolbarLabel: z.string().nullable(),
	keys: z.array(z.string()),
	availability: z.discriminatedUnion('kind', [
		z.strictObject({ kind: z.literal('available') }),
		z.strictObject({ kind: z.literal('unavailable'), reason: z.string().min(1) }),
	]),
	checked: z.boolean().nullable(),
});
const registry = z.strictObject({
	scope: z.string().min(1),
	primary: z.discriminatedUnion('kind', [
		z.strictObject({ kind: z.literal('observed'), action: z.string().nullable() }),
		z.strictObject({ kind: z.literal('notExposed') }),
	]),
	actions: z.array(action),
});
const nonnegative = z.number().int().nonnegative().max(Number.MAX_SAFE_INTEGER);
export const showcaseStateSchema = z.strictObject({
	observation: nonnegative.min(1),
	inputOrdinal: nonnegative,
	cause: z.enum(['input', 'fixture', 'frame']),
	completionReady: z.boolean(),
	selection: z.string().nullable(),
	detailSelection: z.string().nullable(),
	filter: z.string(),
	filterCursor: nonnegative,
	draft: z.string(),
	cursor: nonnegative,
	committed: z.string(),
	choice: z.string().nullable(),
	query: z.string(),
	highlight: z.string().nullable(),
	effects: z.array(z.strictObject({ action: z.string(), target: z.string().nullable() })),
	setting: z.enum(['all', 'local']),
	title: z.string(),
	primaryTitle: z.string(),
	startEnabled: z.boolean(),
	quitEnabled: z.boolean(),
	alternatePrimary: z.boolean(),
	completionCount: nonnegative,
	pendingCompletion: z.boolean(),
	surface,
	focus: z.string().nullable(),
	focusTargets: z.array(
		z.strictObject({
			targetId: z.string(),
			role: z.enum(['focusable', 'pointerOnly']),
			eligible: z.boolean(),
		}),
	),
	overlays: z.array(
		z.strictObject({
			overlayId: z.string().regex(/^[1-9][0-9]*$/u),
			kind: z.enum(['menu', 'dialog', 'dropdown']),
			primary: z.string().nullable(),
		}),
	),
	scroll: z.strictObject({
		main: nonnegative.nullable(),
		dialog: nonnegative.nullable(),
		detail: nonnegative.nullable(),
	}),
	viewport: z.tuple([nonnegative, nonnegative]),
	hitMap: z
		.strictObject({
			frame: nonnegative,
			targets: z.array(
				z.strictObject({
					surface,
					target: z.string(),
					kind: z.enum(['target', 'scrollArea']),
					cells: z.array(cell),
				}),
			),
			surfaces: z.array(z.strictObject({ surface, cells: z.array(cell) })),
		})
		.nullable(),
	registries: z.strictObject({
		screen: registry.nullable(),
		overlays: z.array(z.strictObject({ surface, current: registry, base: registry.nullable() })),
	}),
});
export type ShowcaseState = z.infer<typeof showcaseStateSchema>;
export type ShowcaseRegistry = z.infer<typeof registry>;
export class PrerequisiteGap extends Error {
	public override readonly name = 'PrerequisiteGap';
}
export class ScenarioMismatch extends Error {
	public override readonly name = 'ScenarioMismatch';
	public readonly observed: unknown;
	public constructor(message: string, observed: unknown) {
		super(message);
		this.observed = observed;
	}
}
export function requireObservation(
	condition: boolean,
	message: string,
	observed: unknown,
): asserts condition {
	if (!condition) throw new ScenarioMismatch(message, observed);
}
export function childTarget(parent: string, suffix: string): string {
	return `${Buffer.byteLength(parent)}:${parent}:${suffix}`;
}
