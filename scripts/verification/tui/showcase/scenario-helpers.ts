import { canonicalJson } from '../evidence/source-binding.ts';
import { cellText } from '../terminal/terminal-grid.ts';
import { childTarget, PrerequisiteGap, requireObservation } from './contracts.ts';
import type { ShowcaseRegistry, ShowcaseState } from './contracts.ts';
import type { ShowcaseRig } from './rig.ts';
export function currentRegistry(state: ShowcaseState): ShowcaseRegistry {
	const registry =
		state.surface === 'screen'
			? state.registries.screen
			: state.registries.overlays.find((entry) => entry.surface === state.surface)?.current;
	if (registry === null || registry === undefined)
		throw new PrerequisiteGap('current registry not exposed');
	return registry;
}
export function actionPresentation(
	state: ShowcaseState,
	actionId: string,
): ShowcaseRegistry['actions'][number] {
	const action = currentRegistry(state).actions.find(
		(candidate) => candidate.actionId === actionId,
	);
	if (action === undefined) throw new PrerequisiteGap('declared action absent: ' + actionId);
	return action;
}
export function effectCount(state: ShowcaseState, action: string): number {
	return state.effects.filter((effect) => effect.action === action).length;
}
export function visibleTarget(
	state: Pick<ShowcaseState, 'hitMap' | 'surface'>,
	target: string,
): boolean {
	return (
		state.hitMap?.targets.some(
			(region) =>
				region.surface === state.surface && region.target === target && region.cells.length > 0,
		) === true
	);
}
export function eligibleVisibleFocus(
	state: Pick<ShowcaseState, 'focus' | 'focusTargets' | 'hitMap' | 'surface'>,
): void {
	// ListSelectionControl owns a keyboard target while ScrollList publishes
	// its distinct scroll child. Buttons/fields publish their direct target.
	const paintedTarget =
		state.focus === 'sample-list' || state.focus === 'detail-list'
			? childTarget(state.focus, 'scroll')
			: state.focus;
	requireObservation(
		state.focus !== null &&
			state.focusTargets.some(
				(target) =>
					target.targetId === state.focus && target.eligible && target.role === 'focusable',
			) &&
			paintedTarget !== null &&
			visibleTarget(state, paintedTarget),
		'focused control is eligible and visible immediately after redraw',
		state,
	);
}
export function sameFields(
	before: ShowcaseState,
	after: ShowcaseState,
	fields: readonly (keyof ShowcaseState)[],
	message: string,
): void {
	for (const field of fields)
		requireObservation(
			canonicalJson(before[field]) === canonicalJson(after[field]),
			message + ' ' + field,
			{ before: before[field], after: after[field] },
		);
}
export async function escapeOverlay(rig: ShowcaseRig): Promise<void> {
	const state = rig.state();
	const helpBody = childTarget('action:help', 'help-body');
	const target =
		currentRegistry(state).scope === 'help' && state.focus === helpBody ? helpBody : null;
	await rig.key('Escape', rig.route(null, target));
}
export async function openForm(rig: ShowcaseRig): Promise<void> {
	await rig.key('Ctrl+N', rig.route('form'));
	requireObservation(
		rig.state().overlays.at(-1)?.kind === 'dialog',
		'form action opens one real dialog',
		rig.state().overlays,
	);
}
export async function openChoice(rig: ShowcaseRig, field = 'choice'): Promise<void> {
	await rig.focus(field);
	await rig.key(
		'Enter',
		rig.route(field === 'choice' ? 'choice-open' : 'screen-choice-open', field),
	);
	requireObservation(
		rig.state().overlays.at(-1)?.kind === 'dropdown',
		'field Enter opens a real dropdown',
		rig.state().overlays,
	);
}
export function dropdownFilter(field = 'choice'): string {
	return childTarget(field, 'filter');
}
export function surfaceBounds(state: Pick<ShowcaseState, 'hitMap' | 'surface'>): {
	readonly left: number;
	readonly right: number;
	readonly top: number;
	readonly bottom: number;
} {
	const cells = state.hitMap?.surfaces.find((entry) => entry.surface === state.surface)?.cells;
	if (cells === undefined || cells.length === 0)
		throw new PrerequisiteGap('current surface footprint unavailable');
	return {
		left: Math.min(...cells.map((cell) => cell[0])),
		right: Math.max(...cells.map((cell) => cell[0])),
		top: Math.min(...cells.map((cell) => cell[1])),
		bottom: Math.max(...cells.map((cell) => cell[1])),
	};
}

export async function showHelpConvention(rig: ShowcaseRig, body: string): Promise<void> {
	await rig.key('End', rig.route(null, body));
	const reveal = async (remaining: number): Promise<void> => {
		const bounds = surfaceBounds(rig.drawState());
		const words = rig
			.grid()
			.cells.slice(bounds.top + 1, bounds.bottom)
			.map((row) =>
				row
					.slice(bounds.left + 1, bounds.right)
					.map(cellText)
					.join('')
					.trim()
					.replace(/[│█]+$/u, '')
					.trim(),
			)
			.join(' ')
			.replace(/\s+/gu, ' ');
		if (words.includes('never quit')) return;
		if (remaining === 0)
			throw new PrerequisiteGap('Help convention not found through bounded real scroll');
		await rig.key('Up', rig.route(null, body));
		await reveal(remaining - 1);
	};
	await reveal(256);
}
