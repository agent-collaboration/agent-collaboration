import { cellText } from '../terminal-grid.ts';
import { childTarget, PrerequisiteGap, requireObservation } from './contracts.ts';
import type { ShowcaseRig } from './rig.ts';
import { effectCount, escapeOverlay, visibleTarget } from './scenario-helpers.ts';

export async function rowPaging(rig: ShowcaseRig): Promise<void> {
	await rig.key('Down', rig.route(null, 'sample-list'));
	requireObservation(
		rig.state().selection === 'sample-01',
		'Down changes keyed selection',
		rig.state().selection,
	);
	await rig.key('PageDown', rig.route(null, 'sample-list'));
	requireObservation(
		rig.state().selection ===
			`sample-${String(Math.min(35, 1 + rig.grid().rows - 7)).padStart(2, '0')}`,
		'PageDown uses current row budget',
		rig.state().selection,
	);
	await rig.key('PageUp', rig.route(null, 'sample-list'));
	await rig.key('End', rig.route(null, 'sample-list'));
	await rig.awaitVisibleTarget(childTarget('sample-list', 'row:sample-35'));
	requireObservation(
		rig.state().selection === 'sample-35' &&
			visibleTarget(rig.drawState(), childTarget('sample-list', 'row:sample-35')),
		'End reveals the last selected key',
		rig.state(),
	);
	await rig.key('Home', rig.route(null, 'sample-list'));
	await rig.wheel(childTarget('sample-list', 'scroll'), 'down');
	requireObservation(
		rig.state().selection === 'sample-01',
		'RowPaging wheel changes selection',
		rig.state().selection,
	);
	const effects = rig.state().effects.length;
	await rig.click(childTarget('sample-list', 'row:sample-02'), null);
	requireObservation(
		rig.state().selection === 'sample-02' &&
			rig.state().focus === 'sample-list' &&
			rig.state().effects.length === effects,
		'row click selects/focuses without activation',
		rig.state(),
	);
	await rig.key('Enter', rig.route('details', 'sample-list'));
	await rig.click('action:open', 'open');
	requireObservation(
		rig.state().effects.at(-1)?.target === 'sample-02',
		'Enter/Details Open records exact keyed row',
		rig.state().effects,
	);
	await escapeOverlay(rig);
	await rig.focus('sample-list');
	await rig.key('Tab');
	requireObservation(
		!rig.state().focus?.includes(':row:'),
		'Tab exits list once without pointer rows',
		rig.state().focus,
	);
	await rig.menuAction('reorder-rows');
	requireObservation(
		rig.state().selection === 'sample-02',
		'reorder retains stable key',
		rig.state().selection,
	);
	await rig.menuAction('remove-row');
	requireObservation(
		rig.state().selection === 'sample-01',
		'removal selects the prior index in the reordered rows',
		rig.state().selection,
	);
	await rig.focus('sample-list');
	await rig.key('End', rig.route(null, 'sample-list'));
	await rig.menuAction('remove-row');
	requireObservation(
		rig.state().selection === 'sample-01',
		'removing last selects last remaining row',
		rig.state().selection,
	);
	await rig.menuAction('empty-rows');
	requireObservation(
		rig.state().selection === null,
		'empty list has no fabricated identity',
		rig.state().selection,
	);
	rig.requireText('No match');
	requireObservation(
		effectCount(rig.state(), 'open') === 1,
		'navigation and row clicks add no activations',
		rig.state().effects,
	);
	await rig.capture('row-paging-empty');
}
export async function contentScroll(rig: ShowcaseRig): Promise<void> {
	await rig.key('F5', rig.route('fill'));
	await rig.focus('action:close');
	await rig.key('End', rig.route(null, 'action:close'));
	await rig.awaitVisibleTarget(childTarget('detail-list', 'row:sample-00'));
	await rig.focus('detail-list');
	const selection = rig.state().detailSelection;
	await rig.key('End', rig.route(null, 'detail-list'));
	requireObservation(
		rig.state().detailSelection === selection && (rig.state().scroll.detail ?? 0) > 0,
		'ContentScroll End scrolls while retaining selection',
		rig.state(),
	);
	await rig.key('PageUp', rig.route(null, 'detail-list'));
	await rig.key('Home', rig.route(null, 'detail-list'));
	await rig.wheel(childTarget('detail-list', 'scroll'), 'down');
	requireObservation(
		rig.state().detailSelection === selection,
		'ContentScroll page/home/wheel preserve selection',
		rig.state(),
	);
	await rig.key('Down', rig.route(null, 'detail-list'));
	requireObservation(
		rig.state().detailSelection === 'sample-01',
		'arrow changes keyed content selection',
		rig.state(),
	);
	const row = rig
		.state()
		.hitMap?.targets.find(
			(target) =>
				target.surface === rig.state().surface &&
				target.target === childTarget('detail-list', 'row:sample-01'),
		);
	if (row === undefined)
		throw new PrerequisiteGap('selected group row is clipped from the nested viewport');
	const rows = [...new Set(row.cells.map((cell) => cell[1]))].toSorted(
		(left, right) => left - right,
	);
	requireObservation(rows.length >= 2, 'selected row exposes heading and selectable content', rows);
	const column = Math.min(...row.cells.map((cell) => cell[0]));
	const heading = rig.grid().cells[rows[0] ?? -1]?.[column];
	const selected = rig.grid().cells[rows[1] ?? -1]?.[column];
	requireObservation(
		heading !== undefined &&
			selected !== undefined &&
			cellText(heading).trim() === '' &&
			cellText(selected).trim() !== '',
		'marker_offset places marker below the group heading',
		{ heading, selected },
	);
	await rig.capture('content-scroll-marker');
	await rig.focus('action:close');
	const hover = rig.point(childTarget('detail-list', 'row:sample-02'));
	const effects = rig.state().effects.length;
	await rig.rawKey({
		bytes: Buffer.from(`\x1b[<35;${hover.column + 1};${hover.row + 1}M`),
		event: { kind: 'mouse', mouse: { kind: 'moved' }, ...hover, modifiers: [] },
		route: rig.route(null, childTarget('detail-list', 'row:sample-02')),
		label: 'opted-in list hover-selection supplement',
	});
	requireObservation(
		rig.state().detailSelection === 'sample-02' && rig.state().effects.length === effects,
		'opted-in hover selects without activation while list is unfocused',
		rig.state(),
	);
	await rig.capture('hover-selected-unfocused');
	await rig.focus('detail-list');
	await rig.capture('focused-selected');
	await escapeOverlay(rig);
}
