import { cellText } from '../terminal-grid.ts';
import { childTarget, PrerequisiteGap, requireObservation } from './contracts.ts';
import type { ShowcaseRig } from './rig.ts';
import {
	dropdownFilter,
	effectCount,
	escapeOverlay,
	openChoice,
	openForm,
	sameFields,
	surfaceBounds,
} from './scenario-helpers.ts';

async function deleteQuery(rig: ShowcaseRig): Promise<void> {
	if (rig.state().query === '') return;
	await rig.rawKey({
		bytes: Buffer.from('\x7f'),
		event: {
			kind: 'key',
			code: { kind: 'fixed', name: process.platform === 'darwin' ? 'Delete' : 'Backspace' },
			modifiers: [],
			phase: 'press',
		},
		route: rig.route(null, dropdownFilter()),
		label: 'query backspace supplement',
	});
	await deleteQuery(rig);
}
export async function comboPickCancel(rig: ShowcaseRig): Promise<void> {
	await openForm(rig);
	const parent = rig.state();
	await openChoice(rig);
	const bounds = surfaceBounds(rig.drawState());
	const field = parent.hitMap?.targets.find(
		(entry) => entry.surface === parent.surface && entry.target === 'choice',
	);
	const belowField =
		field !== undefined && bounds.top > Math.max(...field.cells.map((cell) => cell[1]));
	requireObservation(
		rig.state().choice === 'beta' && rig.state().highlight === 'beta',
		'current duplicate-label key is marked/highlighted',
		rig.state(),
	);
	await rig.text('Shared', dropdownFilter());
	requireObservation(
		rig.state().highlight === 'beta',
		'still-matching query preserves stable highlight',
		rig.state(),
	);
	await deleteQuery(rig);
	await rig.key('Down', rig.route(null, dropdownFilter()));
	const point = rig.point(childTarget('choice', 'option:alpha'));
	await rig.rawKey({
		bytes: Buffer.from(`\x1b[<35;${point.column + 1};${point.row + 1}M`),
		event: {
			kind: 'mouse',
			mouse: { kind: 'moved' },
			column: point.column,
			row: point.row,
			modifiers: [],
		},
		route: rig.route(null, childTarget('choice', 'option:alpha')),
		label: 'declared SGR hover supplement',
	});
	requireObservation(
		rig.state().highlight === 'alpha',
		'hover resolves option by key',
		rig.state().highlight,
	);
	await rig.click(childTarget('choice', 'option:beta'), null, { closes: true });
	requireObservation(
		rig.state().choice === 'beta' && effectCount(rig.state(), 'pick') === 1,
		'click picks clicked duplicate-label key rather than stale highlight',
		rig.state(),
	);
	await openChoice(rig);
	await rig.key('Down', rig.route(null, dropdownFilter()));
	await escapeOverlay(rig);
	requireObservation(
		rig.state().choice === 'beta' && effectCount(rig.state(), 'pick') === 1,
		'Esc cancels changed highlight without committing',
		rig.state(),
	);
	await rig.capture('combo-cancel');
	await escapeOverlay(rig);
	if (!belowField)
		throw new PrerequisiteGap(
			'pick/cancel observed; below-field placement remains unobserved with the declared fixed AtCells fixture pending A4',
		);
}
async function staged(rig: ShowcaseRig, action: string): Promise<void> {
	await rig.menuAction(action);
	await openForm(rig);
	await openChoice(rig);
}
export async function comboOptionUpdates(rig: ShowcaseRig): Promise<void> {
	await staged(rig, 'stage-reorder');
	await rig.text('Shared', dropdownFilter());
	requireObservation(
		rig.state().highlight === 'beta',
		'typing that still matches preserves highlight',
		rig.state(),
	);
	await rig.completeFixture();
	requireObservation(
		rig.state().highlight === 'beta',
		'external reorder preserves highlighted key',
		rig.state(),
	);
	await deleteQuery(rig);
	await rig.text('Long', dropdownFilter());
	requireObservation(
		rig.state().highlight === 'option-17',
		'typing that hides highlight selects first matching key in current order',
		rig.state(),
	);
	await escapeOverlay(rig);
	await escapeOverlay(rig);
	await staged(rig, 'stage-remove');
	await rig.completeFixture();
	requireObservation(
		rig.state().highlight === null,
		'external removal clears highlight',
		rig.state(),
	);
	const before = rig.state();
	await rig.key('Enter', rig.route(null, dropdownFilter()));
	sameFields(
		before,
		rig.state(),
		['choice', 'effects', 'overlays'],
		'immediate Enter after removal picks nothing',
	);
	await rig.key('Down', rig.route(null, dropdownFilter()));
	requireObservation(
		rig.state().highlight !== null,
		'arrow establishes a new highlight',
		rig.state(),
	);
	await escapeOverlay(rig);
	await escapeOverlay(rig);
	await staged(rig, 'stage-add');
	await rig.completeFixture();
	await rig.text('Added', dropdownFilter());
	await rig.click(childTarget('choice', 'option:added'), null, { closes: true });
	requireObservation(
		rig.state().choice === 'added',
		'newly added option is present in event-time pointer model',
		rig.state(),
	);
	await rig.capture('added-option-picked');
	await escapeOverlay(rig);
}
export async function comboUnavailable(rig: ShowcaseRig): Promise<void> {
	await openForm(rig);
	await openChoice(rig);
	await rig.text('Unavailable', dropdownFilter());
	requireObservation(
		rig.state().highlight === 'unavailable',
		'unavailable option remains highlightable',
		rig.state(),
	);
	const seen = new Set<string>();
	const revealReason = async (remaining: number): Promise<void> => {
		const state = rig.drawState();
		const option = state.hitMap?.targets.find(
			(region) =>
				region.surface === state.surface &&
				region.target === childTarget('choice', 'option:unavailable'),
		);
		const words =
			option?.cells
				.map(([column, row]) => {
					const cell = rig.grid().cells[row]?.[column];
					return cell === undefined ? '' : cellText(cell);
				})
				.join('')
				.replace(/\s/gu, '') ?? '';
		for (const word of ['Requires', 'sample', 'permission'])
			if (words.includes(word)) seen.add(word);
		if (seen.size === 3) return;
		if (remaining === 0)
			throw new PrerequisiteGap('bounded option scroll could not expose every reason word');
		await rig.capture('unavailable-reason-' + remaining);
		const after = await rig.wheel(childTarget('choice', 'scroll'), 'down', false);
		requireObservation(
			after.hitMap === null,
			'every declared reason word is visible before the actual option scroll ends',
			{ seen: [...seen] },
		);
		await rig.settleFrame();
		await revealReason(remaining - 1);
	};
	await revealReason(64);
	const before = rig.state();
	await rig.key('Enter', rig.route(null, dropdownFilter()));
	await rig.click(childTarget('choice', 'option:unavailable'), null);
	sameFields(
		before,
		rig.state(),
		['choice', 'effects', 'overlays'],
		'unavailable Enter/click cannot commit',
	);
	await deleteQuery(rig);
	await rig.text('unmatchedzz', dropdownFilter());
	rig.requireText('No match');
	requireObservation(
		rig.state().highlight === null,
		'empty matches have no fabricated highlight',
		rig.state(),
	);
	await deleteQuery(rig);
	requireObservation(
		rig.state().highlight !== null,
		'clearing query recovers normal options',
		rig.state(),
	);
	await rig.capture('unavailable-no-match');
	await escapeOverlay(rig);
	await escapeOverlay(rig);
}
export async function compoundDropdown(rig: ShowcaseRig): Promise<void> {
	let startsBeforeChord = 0;
	if (rig.keys === 'legacy') {
		await openForm(rig);
		await openChoice(rig);
		await rig.key('Down', rig.route(null, dropdownFilter()));
		const unchangedChoice = rig.state().choice;
		await escapeOverlay(rig);
		await rig.focus('action:start');
		await rig.key('Enter', rig.route('start', 'action:start'));
		requireObservation(
			rig.state().overlays.length === 1 &&
				rig.state().choice === unchangedChoice &&
				effectCount(rig.state(), 'start') === 1,
			'legacy Esc then Tab plus Enter starts once without committing dropdown highlight',
			rig.state(),
		);
		await rig.capture('legacy-compound-primary-fallback');
		await escapeOverlay(rig);
		await rig.key('Ctrl+G', rig.route('enable'));
		startsBeforeChord = 1;
	}
	await openForm(rig);
	await openChoice(rig);
	await rig.key('Down', rig.route(null, dropdownFilter()));
	const before = rig.state();
	const parent = before.overlays.at(-2);
	requireObservation(parent !== undefined, 'dropdown has an owning dialog', before.overlays);
	await rig.key('Ctrl+Enter', {
		surface: 'overlay:' + parent.overlayId,
		target: null,
		action: 'start',
	});
	requireObservation(
		rig.state().overlays.length === 1 &&
			effectCount(rig.state(), 'start') === startsBeforeChord + 1 &&
			rig.state().choice === before.choice,
		'Ctrl+Enter closes only dropdown and starts once without picking highlight',
		rig.state(),
	);
	await openChoice(rig);
	await escapeOverlay(rig);
	requireObservation(
		rig.state().overlays.length === 1,
		'first Esc closes only dropdown',
		rig.state().overlays,
	);
	await escapeOverlay(rig);
	requireObservation(
		rig.state().overlays.length === 0 &&
			effectCount(rig.state(), 'start') === startsBeforeChord + 1,
		'second Esc closes dialog without another Start',
		rig.state(),
	);
	await openForm(rig);
	await openChoice(rig);
	await rig.key('Down', rig.route(null, dropdownFilter()));
	const highlighted = rig.state().highlight;
	await rig.key('Enter', rig.route(null, dropdownFilter()));
	requireObservation(
		rig.state().choice === highlighted && rig.state().overlays.length === 1,
		'plain Enter picks within dropdown',
		rig.state(),
	);
	await escapeOverlay(rig);
}
export async function anchoredResize(rig: ShowcaseRig): Promise<void> {
	await rig.resize(120, 36);
	await openForm(rig);
	const oldField = rig.point('choice');
	await openChoice(rig);
	const old = rig.point(childTarget('choice', 'option:alpha'));
	await rig.resize(76, 30);
	const dropdown = rig.state();
	const parent = dropdown.overlays.at(-2);
	const field = dropdown.hitMap?.targets.find(
		(entry) => entry.surface === 'overlay:' + parent?.overlayId && entry.target === 'choice',
	);
	const bounds = surfaceBounds(dropdown);
	requireObservation(
		field !== undefined &&
			bounds.top > Math.max(...field.cells.map((cell) => cell[1])) &&
			bounds.left >= Math.min(...field.cells.map((cell) => cell[0])) - 1,
		'resized dropdown remains attached below its moved field',
		{ oldField, old, bounds, field },
	);
	const current = rig.point(childTarget('choice', 'option:alpha'));
	requireObservation(
		current.column !== old.column || current.row !== old.row,
		'same-child resize moves drawn option geometry',
		{ old, current },
	);
	await rig.click(childTarget('choice', 'option:alpha'), null, { closes: true });
	requireObservation(
		rig.state().choice === 'alpha',
		'new drawn option coordinates pick intended stable key',
		rig.state(),
	);
	await rig.capture('anchored-resized');
	await escapeOverlay(rig);
	await rig.resize(120, 36);
}
