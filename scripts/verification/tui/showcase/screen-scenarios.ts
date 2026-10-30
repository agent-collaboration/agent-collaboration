import { canonicalJson } from '../source-binding.ts';
import { childTarget, requireObservation } from './contracts.ts';
import { expectedInput, expectedKey } from './input-expectations.ts';
import type { ShowcaseRig } from './rig.ts';
import {
	actionPresentation,
	currentRegistry,
	effectCount,
	eligibleVisibleFocus,
	escapeOverlay,
	openChoice,
	openForm,
	sameFields,
	showHelpConvention,
	visibleTarget,
} from './scenario-helpers.ts';

export async function toolbarDropOff(rig: ShowcaseRig): Promise<void> {
	await rig.resize(120, 36);
	await rig.focus('action:record');
	await rig.resize(32, 24);
	eligibleVisibleFocus(rig.drawState());
	requireObservation(
		!visibleTarget(rig.drawState(), 'action:record'),
		'rightmost toolbar button drops at narrow width',
		rig.state().hitMap,
	);
	await rig.menu();
	requireObservation(
		actionPresentation(rig.state(), 'record').title === 'Record exact selection',
		'dropped action remains in Menu',
		currentRegistry(rig.state()),
	);
	await escapeOverlay(rig);
	await rig.resize(120, 36);
	await rig.focus('action:record');
	await rig.key('Ctrl+R', rig.route('live-title'));
	await rig.key('Ctrl+Enter', rig.route('record'));
	eligibleVisibleFocus(rig.drawState());
	await rig.menu();
	const presented = actionPresentation(rig.state(), 'record');
	requireObservation(
		presented.title.includes('longer live title') &&
			presented.availability.kind === 'unavailable' &&
			presented.toolbarLabel === 'Record',
		'live text/reason remain projected after toolbar drop-off',
		presented,
	);
	await escapeOverlay(rig);
	await rig.capture('toolbar-drop-off');
}
export async function menuProjections(rig: ShowcaseRig): Promise<void> {
	const screen = rig.state().registries.screen;
	requireObservation(screen !== null, 'screen registry is available', screen);
	await rig.menu();
	const entries = currentRegistry(rig.state()).actions;
	const expected = screen.actions.filter(
		(action) => action.actionId !== 'menu' && action.actionId !== 'escape',
	);
	requireObservation(
		canonicalJson(entries) === canonicalJson(expected),
		'Menu projects exact current titles, keys, availability and checked settings',
		{ expected, entries },
	);
	requireObservation(
		entries.every((action) => action.actionId !== 'menu'),
		'Menu excludes itself',
		entries,
	);
	await rig.capture('menu-keyboard');
	await escapeOverlay(rig);
	await rig.click('action:menu', 'menu', { opens: true });
	await rig.menuAction('local', 'mouse');
	requireObservation(
		rig.state().setting === 'local',
		'mouse-only menu action runs shared setting',
		rig.state(),
	);
	await rig.focus('action:menu');
	await rig.key('Enter', rig.route('menu', 'action:menu'));
	await rig.menuAction('empty-rows');
	requireObservation(
		rig.state().selection === null,
		'long menu reaches and activates its final fixture actions',
		rig.state(),
	);
	await rig.key('F1', rig.route('help'));
	rig.requireText('Help');
	await showHelpConvention(rig, childTarget('action:help', 'help-body'));
	await escapeOverlay(rig);
}
export async function segmentedSettings(rig: ShowcaseRig): Promise<void> {
	await rig.click('action:local', 'local');
	requireObservation(rig.state().setting === 'local', 'click selects Local', rig.state().setting);
	await rig.focus('action:all');
	await rig.key('Right');
	requireObservation(
		rig.state().focus === 'action:local',
		'grouped arrow reaches next segment',
		rig.state().focus,
	);
	await rig.key('Enter', rig.route('local', 'action:local'));
	await rig.key('Ctrl+E', rig.route('all'));
	await rig.menuAction('local');
	await rig.menu();
	requireObservation(
		actionPresentation(rig.state(), 'local').checked === true &&
			actionPresentation(rig.state(), 'all').checked === false,
		'menu checks reflect the shared setting after reopen',
		currentRegistry(rig.state()),
	);
	await rig.menuAction('segment-availability');
	const before = rig.state();
	await rig.key('Ctrl+F', rig.route(), false);
	sameFields(before, rig.state(), ['setting', 'effects'], 'disabled bound segment changes nothing');
	await rig.menu();
	requireObservation(
		actionPresentation(rig.state(), 'local').availability.kind === 'unavailable',
		'unavailable segment carries its reason',
		currentRegistry(rig.state()),
	);
	await rig.capture('disabled-segment');
	await escapeOverlay(rig);
}
export async function fieldEditing(rig: ShowcaseRig): Promise<void> {
	const hover = rig.point('filter');
	const beforeHover = rig.grid().cells[hover.row]?.[hover.column];
	await rig.rawKey({
		bytes: Buffer.from(`\x1b[<35;${hover.column + 1};${hover.row + 1}M`),
		event: { kind: 'mouse', mouse: { kind: 'moved' }, ...hover, modifiers: [] },
		route: rig.route(),
		label: 'declared field-hover supplement',
	});
	requireObservation(
		beforeHover?.bgRgb !== rig.grid().cells[hover.row]?.[hover.column]?.bgRgb,
		'unfocused field hover is visibly distinct',
		{ beforeHover, after: rig.grid().cells[hover.row]?.[hover.column] },
	);
	await rig.focus('filter');
	await rig.text('ab界', 'filter');
	await rig.key('Left', rig.route(null, 'filter'));
	await rig.text('/', 'filter');
	await rig.paste('✉️', 'filter');
	requireObservation(
		rig.state().filter === 'ab/✉️界' && rig.state().filterCursor === 5,
		'Unicode insert and paste use the current character cursor',
		rig.state(),
	);
	await rig.key('Home', rig.route(null, 'filter'));
	await rig.key('Right', rig.route(null, 'filter'));
	await rig.rawKey({
		bytes: Buffer.from('\x7f'),
		event: {
			kind: 'key',
			code: { kind: 'fixed', name: process.platform === 'darwin' ? 'Delete' : 'Backspace' },
			modifiers: [],
			phase: 'press',
		},
		route: rig.route(null, 'filter'),
		label: 'legacy DEL editing supplement',
	});
	await rig.rawKey({
		bytes: Buffer.from('\x1b[3~'),
		event: {
			kind: 'key',
			code: { kind: 'fixed', name: process.platform === 'darwin' ? 'Fwd Del' : 'Del' },
			modifiers: [],
			phase: 'press',
		},
		route: rig.route(null, 'filter'),
		label: 'VT forward-delete supplement',
	});
	requireObservation(
		rig.state().filter === '/✉️界' && rig.state().filterCursor === 0,
		'Backspace/Delete remove characters on opposite sides of cursor',
		rig.state(),
	);
	await rig.burst({
		bytes: Buffer.from('\x1b[Fx\x1b[200~界\x1b[201~'),
		expected: [
			expectedKey('End', rig.route(null, 'filter')),
			expectedInput(
				{ kind: 'key', code: { kind: 'text' }, modifiers: [], phase: 'press' },
				rig.route(null, 'filter'),
			),
			expectedInput({ kind: 'other', event: 'bracketedPaste' }, rig.route(null, 'filter')),
		],
		label: 'movement insertion paste in one PTY write',
	});
	requireObservation(
		rig.state().filter === '/✉️界x界',
		'same-write cursor movement precedes insertion and paste',
		rig.state(),
	);
	await rig.key('Ctrl+N', rig.route('form'));
	await rig.focus('draft');
	await rig.key('Home', rig.route(null, 'draft'));
	await rig.text('界/', 'draft');
	requireObservation(
		rig.state().draft.startsWith('界//sample/界/project'),
		'TextField owns printable slash and Unicode editing',
		rig.state().draft,
	);
	await rig.capture('unicode-editing');
	await escapeOverlay(rig);
	await rig.menuAction('field-availability');
	const disabled = rig.state();
	await rig.click('filter', null, { ignored: true });
	await rig.rawKey({
		bytes: Buffer.from('z'),
		event: { kind: 'key', code: { kind: 'text' }, modifiers: [], phase: 'press' },
		route: rig.route(),
		redraw: false,
		label: 'disabled-field printable input',
	});
	sameFields(
		disabled,
		rig.state(),
		['filter', 'effects'],
		'unavailable field cannot receive click or text',
	);
	rig.requireText('Sample field disabled');
	await rig.capture('disabled-field');
}
export async function mainActionMeanings(rig: ShowcaseRig): Promise<void> {
	await rig.key('Down', rig.route(null, 'sample-list'));
	const selected = rig.state().selection;
	await rig.key('Enter', rig.route('details', 'sample-list'));
	requireObservation(
		effectCount(rig.state(), 'open') === 0 && rig.state().overlays.length === 1,
		'ordinary Enter inspects without recording Open',
		rig.state(),
	);
	await rig.click('action:open', 'open');
	requireObservation(
		rig.state().effects.at(-1)?.target === selected,
		'Details Open fallback records the exact keyed selection',
		rig.state().effects,
	);
	await rig.click('action:fork', 'fork', { opens: true });
	requireObservation(
		rig.state().overlays.length === 2,
		'Details Fork fallback opens confirmation',
		rig.state().overlays,
	);
	await escapeOverlay(rig);
	await escapeOverlay(rig);
	if (rig.keys === 'legacy') return;
	await rig.key('Ctrl+Enter', rig.route('record'));
	requireObservation(
		rig.state().effects.at(-1)?.target === selected,
		'modified Enter records exact selection',
		rig.state().effects,
	);
	await rig.key('Ctrl+Shift+Enter', rig.route('fork'));
	await escapeOverlay(rig);
}
export async function escapeAndQuit(rig: ShowcaseRig): Promise<void> {
	await rig.focus('filter');
	await rig.text('Same', 'filter');
	await rig.key('Ctrl+F', rig.route('local'));
	await rig.key('Escape', rig.route(null, 'filter'));
	requireObservation(
		rig.state().filter === '' && rig.state().setting === 'local',
		'first Esc clears only the nonempty filter',
		rig.state(),
	);
	await rig.key('Escape', rig.route('escape'));
	requireObservation(rig.state().setting === 'all', 'second Esc resets filters', rig.state());
	await rig.key('Escape', rig.route('escape'));
	rig.sameChild();
	await rig.key('Ctrl+D', rig.route('guard'));
	await rig.key('Ctrl+C', { surface: 'screen', target: null, action: null }, false);
	await rig.menu();
	await rig.key('Ctrl+C', { surface: 'screen', target: null, action: null }, false);
	await escapeOverlay(rig);
	await openChoice(rig, 'screen-choice');
	await rig.key('Ctrl+C', { surface: 'screen', target: null, action: null }, false);
	await escapeOverlay(rig);
	await openForm(rig);
	await rig.key('Ctrl+C', { surface: 'screen', target: null, action: null }, false);
	rig.sameChild();
	requireObservation(
		effectCount(rig.state(), 'quit') === 0,
		'registered guard suppresses global Quit on each surface',
		rig.state().effects,
	);
	await escapeOverlay(rig);
	await rig.key('Ctrl+D', rig.route('guard'));
}
export async function liveActions(rig: ShowcaseRig): Promise<void> {
	await rig.key('Ctrl+R', rig.route('live-title'));
	await rig.menu();
	const presentation = actionPresentation(rig.state(), 'record');
	requireObservation(
		presentation.title === rig.state().title && presentation.toolbarLabel === 'Record',
		'full title and optional short label remain separate',
		presentation,
	);
	await escapeOverlay(rig);
	const bytes =
		rig.keys === 'kitty'
			? Buffer.from('\x1b[13;5u\x1b[13;5u')
			: Buffer.from('\x1b[13;5u\x1b[13;5u');
	await rig.burst({
		bytes,
		expected: [
			expectedKey('Ctrl+Enter', rig.route('record')),
			expectedKey('Ctrl+Enter', rig.route()),
		],
		label: 'two Presses in one transport burst',
	});
	requireObservation(
		effectCount(rig.state(), 'open') === 1 && !rig.state().startEnabled,
		'first activation disables the second Press',
		rig.state(),
	);
	await rig.key('Ctrl+Enter', rig.route(), false);
	await rig.menu();
	requireObservation(
		actionPresentation(rig.state(), 'record').availability.kind === 'unavailable',
		'Menu receives fresh disabled reason',
		currentRegistry(rig.state()),
	);
	await rig.menuAction('record', 'mouse');
	requireObservation(
		effectCount(rig.state(), 'open') === 1 && rig.state().overlays.at(-1)?.kind === 'menu',
		'disabled mouse action remains inert and keeps Menu open',
		rig.state(),
	);
	await escapeOverlay(rig);
	await rig.key('F1', rig.route('help'));
	await rig.capture('live-title-help');
	await escapeOverlay(rig);
	await rig.key('Ctrl+G', rig.route('enable'));
	await rig.key('Ctrl+Enter', rig.route('record'));
	requireObservation(
		effectCount(rig.state(), 'open') === 2,
		'reenable restores exactly one activation',
		rig.state().effects,
	);
}
