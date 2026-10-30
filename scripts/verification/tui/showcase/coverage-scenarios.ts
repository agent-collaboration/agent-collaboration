import type { CellData } from '@wterm/core';

import { cellText } from '../terminal-grid.ts';
import { childTarget, PrerequisiteGap, requireObservation } from './contracts.ts';
import { pointForTarget } from './hit-map-points.ts';
import { expectedInput, expectedKey } from './input-expectations.ts';
import type { ShowcaseRig } from './rig.ts';
import {
	eligibleVisibleFocus,
	escapeOverlay,
	openChoice,
	openForm,
	sameFields,
	surfaceBounds,
} from './scenario-helpers.ts';
import type { ScenarioVariant } from './scenario-variants.ts';

export async function outsideClicks(rig: ShowcaseRig): Promise<void> {
	await rig.focus('filter');
	const before = rig.state();
	await rig.menu();
	await rig.clickOutside();
	sameFields(
		before,
		rig.state(),
		['filter', 'draft', 'selection', 'choice', 'effects'],
		'outside menu press/release activates nothing beneath',
	);
	requireObservation(
		rig.state().focus === before.focus,
		'menu restores original target identity',
		rig.state(),
	);
	await rig.menuAction('reorder-rows');
	await rig.menu();
	await rig.clickOutside();
	requireObservation(
		rig.state().focus === before.focus,
		'restore uses target identity after row reorder',
		rig.state(),
	);
	await openChoice(rig, 'screen-choice');
	await rig.clickOutside();
	await openForm(rig);
	const dialog = rig.state();
	await rig.clickOutside();
	requireObservation(
		rig.state().overlays.at(-1)?.overlayId === dialog.overlays.at(-1)?.overlayId,
		'outside dialog click leaves it open',
		rig.state().overlays,
	);
	await escapeOverlay(rig);
	await rig.menuAction('stage-update');
	await rig.focus('action:record');
	await rig.menu();
	await rig.completeFixture();
	await rig.clickOutside();
	eligibleVisibleFocus(rig.drawState());
	const next = rig.nextOverlaySurface();
	const column = rig.grid().columns - 1,
		row = rig.grid().rows - 1;
	const unchanged = rig.state();
	await rig.burst({
		bytes: Buffer.from(`\x1b[21~\x1b[<0;${column + 1};${row + 1}M`),
		expected: [
			expectedKey('F10', rig.route('menu')),
			expectedInput(
				{ kind: 'mouse', mouse: { kind: 'down', button: 'left' }, column, row, modifiers: [] },
				{ surface: next, target: null, action: null },
			),
		],
		redraw: false,
		label: 'menu and outside press in one burst; first-frame ordering observed separately',
	});
	const drewBeforePress = rig.completedDrawBetweenInputs(
		unchanged.inputOrdinal + 1,
		unchanged.inputOrdinal + 2,
	);
	await rig.rawKey({
		bytes: Buffer.from(`\x1b[<0;${column + 1};${row + 1}m`),
		event: { kind: 'mouse', mouse: { kind: 'up', button: 'left' }, column, row, modifiers: [] },
		route: rig.route(),
		redraw: false,
		label: 'release uses the surface that actually owns input after the press',
	});
	sameFields(
		unchanged,
		rig.state(),
		['filter', 'draft', 'selection', 'effects'],
		'first-frame inputs do not click through',
	);
	if (rig.state().overlays.length > 0) await rig.clickOutside();
	await rig.capture('outside-focus-restoration');
	if (drewBeforePress)
		throw new PrerequisiteGap(
			'PTY burst had a completed Menu draw before its outside press; pre-frame path was not exercised',
		);
}
function cellColour(cell: CellData): number {
	return cell.fgRgb ?? cell.fg;
}
export async function coveredSurfaces(rig: ShowcaseRig): Promise<void> {
	await rig.focus('filter');
	await rig.text('Same', 'filter');
	const runCovered = async (kind: 'menu' | 'dialog' | 'dropdown' | 'nested'): Promise<void> => {
		const before = rig.state(),
			beforeDraw = rig.drawState(),
			grid = rig.grid();
		if (kind === 'menu') await rig.menu();
		else if (kind === 'nested') await rig.click('action:fork', 'fork', { opens: true });
		else if (kind === 'dialog') await openForm(rig);
		else await openChoice(rig, 'screen-choice');
		const top = rig.drawState();
		const bounds = surfaceBounds(top);
		let styled = 0;
		for (let row = 0; row < grid.rows; row += 1)
			for (let column = 0; column < grid.columns; column += 1) {
				if (
					column >= Math.max(0, bounds.left - 1) &&
					column <= bounds.right &&
					row >= bounds.top &&
					row <= bounds.bottom
				)
					continue;
				const prior = grid.cells[row]?.[column],
					covered = rig.grid().cells[row]?.[column];
				if (prior === undefined || covered === undefined) continue;
				requireObservation(
					cellText(prior) === cellText(covered),
					'beneath text remains present outside overlay and painted gutter',
					{ kind, column, row, prior, covered },
				);
				if (cellText(prior).trim() !== '' && cellColour(prior) !== cellColour(covered)) styled += 1;
			}
		requireObservation(styled > 0, 'covered components visibly dim instead of disappearing', {
			kind,
			styled,
		});
		const target =
			kind === 'dialog' || kind === 'nested'
				? 'draft'
				: kind === 'dropdown'
					? childTarget('screen-choice', 'filter')
					: null;
		await rig.burst({
			bytes: Buffer.from('z\x1b[200~界\x1b[201~'),
			expected: [
				expectedInput(
					{ kind: 'key', code: { kind: 'text' }, modifiers: [], phase: 'press' },
					rig.route(null, target),
				),
				expectedInput({ kind: 'other', event: 'bracketedPaste' }, rig.route(null, target)),
			],
			redraw: target !== null,
			label: 'text and paste aimed while lower field remains covered',
		});
		await rig.key('Down', rig.route(null, kind === 'dropdown' ? target : null));
		if (kind === 'menu') await rig.wheel('menu-scroll', 'down');
		sameFields(
			before,
			rig.state(),
			kind === 'dialog' || kind === 'nested'
				? ['filter', 'selection', 'choice', 'effects', 'setting']
				: ['filter', 'draft', 'selection', 'choice', 'effects', 'setting'],
			'covered lower surface cannot receive input',
		);
		if (kind === 'nested') {
			const point = pointForTarget(beforeDraw, 'action:open');
			await rig.burst({
				bytes: Buffer.from(
					`\x1b[<0;${point.column + 1};${point.row + 1}M\x1b[<0;${point.column + 1};${point.row + 1}m`,
				),
				expected: [
					expectedInput(
						{ kind: 'mouse', mouse: { kind: 'down', button: 'left' }, ...point, modifiers: [] },
						rig.route(),
					),
					expectedInput(
						{ kind: 'mouse', mouse: { kind: 'up', button: 'left' }, ...point, modifiers: [] },
						rig.route(),
					),
				],
				redraw: false,
				label: 'pointer aimed at covered Details Open button',
			});
			const scroll = beforeDraw.hitMap?.targets.find(
				(region) =>
					region.surface === before.surface &&
					region.kind === 'scrollArea' &&
					region.target === childTarget('detail-list', 'scroll'),
			);
			const exposed = scroll?.cells.find(
				([column, row]) =>
					column < bounds.left - 1 ||
					column > bounds.right ||
					row < bounds.top ||
					row > bounds.bottom,
			);
			if (exposed !== undefined) {
				const [column, row] = exposed;
				await rig.rawKey({
					bytes: Buffer.from(`\x1b[<65;${column + 1};${row + 1}M`),
					event: { kind: 'mouse', mouse: { kind: 'scrollDown' }, column, row, modifiers: [] },
					route: rig.route(),
					redraw: false,
					label: 'wheel aimed at covered Details list',
				});
			}
			requireObservation(
				rig.state().detailSelection === before.detailSelection &&
					rig.state().scroll.detail === before.scroll.detail &&
					rig.state().effects.length === before.effects.length,
				'covered Details roles/list/button retain state under child input',
				rig.state(),
			);
		}
		await rig.capture('covered-' + kind);
		await rig.clickOutside();
		if (kind === 'dialog' || kind === 'nested') await escapeOverlay(rig);
	};
	await runCovered('menu');
	await runCovered('dialog');
	await runCovered('dropdown');
	await openForm(rig);
	await rig.focus('draft');
	const parent = rig.state();
	await openChoice(rig);
	await rig.text('Shared', childTarget('choice', 'filter'));
	await rig.paste(' label', childTarget('choice', 'filter'));
	sameFields(
		parent,
		rig.state(),
		['draft', 'cursor', 'committed', 'choice', 'effects'],
		'dialog draft remains covered by dropdown',
	);
	await escapeOverlay(rig);
	await escapeOverlay(rig);
	await rig.key('F5', rig.route('fill'));
	await rig.focus('action:fork');
	await runCovered('nested');
	await escapeOverlay(rig);
}
function inspectBorder(rig: ShowcaseRig): void {
	const bounds = surfaceBounds(rig.drawState());
	for (let row = bounds.top; row <= bounds.bottom; row += 1) {
		const cell = rig.grid().cells[row]?.[bounds.left];
		const expected = row === bounds.top ? '╭' : row === bounds.bottom ? '╰' : '│';
		requireObservation(
			cell !== undefined && cellText(cell) === expected,
			'actual serialized output retains left border on every row',
			{ row, expected, cell, bounds },
		);
		if (bounds.left > 0) {
			const gutter = rig.grid().cells[row]?.[bounds.left - 1];
			requireObservation(
				gutter !== undefined &&
					cellText(gutter) === ' ' &&
					gutter.width === 1 &&
					!gutter.spacerHead &&
					gutter.bgRgb === cell?.bgRgb,
				'painted gutter clears the wide glyph continuation on every border row',
				{ row, gutter, cell },
			);
		}
	}
}
export async function wideGlyphCoverage(
	rig: ShowcaseRig,
	variant: Extract<ScenarioVariant, { kind: 'wide' }>,
): Promise<void> {
	const { scene, glyph: name } = variant;
	const glyph = { folder: '📂', mail: '✉️', cjk: '界' }[name];
	const sceneIndex = ['menu', 'form', 'dropdown', 'edge'].indexOf(scene),
		glyphIndex = ['folder', 'mail', 'cjk'].indexOf(name);
	const code = '1234567890-='[sceneIndex * 3 + glyphIndex];
	if (code === undefined) throw new PrerequisiteGap('coverage key unavailable');
	const open = async (): Promise<void> => {
		await rig.rawKey({
			bytes: Buffer.from(`\x1b[${code.codePointAt(0)};7u`),
			event: {
				kind: 'key',
				code: { kind: 'shortcut', key: code },
				modifiers: ['control', 'alt'],
				phase: 'press',
			},
			route: rig.route(`coverage-${scene}-${name}`),
			label: 'declared wide-fixture shortcut supplement',
		});
	};
	await open();
	const bounds = surfaceBounds(rig.drawState());
	await escapeOverlay(rig);
	const column = Math.max(0, bounds.left - 1);
	for (let row = bounds.top; row <= bounds.bottom; row += 1) {
		const cell = rig.grid().cells[row]?.[column];
		if (cell === undefined || cellText(cell) !== glyph || cell.width !== 2)
			throw new PrerequisiteGap(
				'wide fixture does not place ' + glyph + ' at actual overlay-left−1 on row ' + row,
			);
	}
	await rig.capture('qualified-wide-fixture');
	await open();
	inspectBorder(rig);
	await rig.capture('wide-open');
	const columns = rig.grid().columns,
		rows = rig.grid().rows;
	await rig.resize(columns === 120 ? 76 : 120, rows === 36 ? 30 : 36);
	inspectBorder(rig);
	await rig.resize(columns, rows);
	inspectBorder(rig);
	await rig.capture('wide-reopened-resized');
	await escapeOverlay(rig);
}
