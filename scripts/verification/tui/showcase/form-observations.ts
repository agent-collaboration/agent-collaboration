/** Physical field columns and scroll-visible words, including wrapped values. */
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { cellText, gridLines, terminalTheme } from '../terminal-grid.ts';
import type { TerminalGrid } from '../terminal-grid.ts';
import { PrerequisiteGap, requireObservation } from './contracts.ts';
import type { ShowcaseState } from './contracts.ts';
import type { ShowcaseRig } from './rig.ts';
import { surfaceBounds } from './scenario-helpers.ts';

type DrawSurface = Pick<ShowcaseState, 'hitMap' | 'surface'>;
function fieldBox(
	state: DrawSurface,
	target: string,
): { readonly column: number; readonly firstRow: number; readonly lastRow: number } {
	const cells = state.hitMap?.targets.find(
		(region) => region.surface === state.surface && region.target === target,
	)?.cells;
	if (cells === undefined || cells.length === 0)
		throw new PrerequisiteGap('sample label/padding budget exposes no field cells for ' + target);
	return {
		column: Math.min(...cells.map(([column]) => column)),
		firstRow: Math.min(...cells.map(([, row]) => row)),
		lastRow: Math.max(...cells.map(([, row]) => row)),
	};
}
export function inspectSectionGeometry(state: DrawSurface, grid: TerminalGrid): number {
	const bounds = surfaceBounds(state),
		lines = gridLines(grid);
	const find = (text: string): number =>
		lines.findIndex((line, row) => row > bounds.top && row < bounds.bottom && line.includes(text));
	const directory = fieldBox(state, 'draft'),
		option = fieldBox(state, 'choice');
	const firstHeading = find('Draft connection'),
		secondHeading = find('Tinted status');
	if (firstHeading < 0 || secondHeading < 0)
		throw new PrerequisiteGap(
			'current form viewport cannot expose both section headings for the gap proof',
		);
	const availableRow = secondHeading + 1;
	const statusOrigin =
		grid.cells[availableRow]?.findIndex(
			(cell, column) =>
				column > bounds.left &&
				column < bounds.right &&
				cellText(cell) === 'A' &&
				cell.fgRgb === terminalTheme.palette[2],
		) ?? -1;
	if (statusOrigin < 0)
		throw new PrerequisiteGap('current form viewport cannot expose the status value origin');
	requireObservation(
		directory.column === option.column && option.column === statusOrigin,
		'both sections share the physical value origin including the selector gutter',
		{ directory, option, statusOrigin },
	);
	requireObservation(
		directory.firstRow === firstHeading + 1 &&
			option.firstRow === directory.lastRow + 1 &&
			secondHeading === option.lastRow + 2,
		'one blank row separates sections, with no interior blank row after wrapped fields',
		{ firstHeading, secondHeading, directory, option },
	);
	return statusOrigin;
}
export function coloredSurfaceWord(
	state: DrawSurface,
	grid: TerminalGrid,
	color: number,
	minimumColumn: number,
): string {
	const bounds = surfaceBounds(state);
	return grid.cells
		.slice(bounds.top + 1, bounds.bottom)
		.flatMap((row) =>
			row
				.slice(minimumColumn, bounds.right)
				.filter((cell) => cell.fgRgb === color)
				.map(cellText),
		)
		.join('')
		.replace(/\s/gu, '');
}
async function advanceBody(rig: ShowcaseRig): Promise<boolean> {
	const before = rig.state().scroll.dialog;
	const after = await rig.wheel('dialog-scroll', 'down', false);
	if (after.scroll.dialog === before) return false;
	await rig.settleFrame();
	return true;
}
export async function observeFormSections(rig: ShowcaseRig): Promise<void> {
	const origin = inspectSectionGeometry(rig.drawState(), rig.grid());
	const good = terminalTheme.palette[2],
		error = terminalTheme.palette[1];
	if (good === undefined || error === undefined)
		throw new PrerequisiteGap('shipped terminal status palette is unavailable');
	await rig.capture('sections-origin');
	await rig.focus('action:close');
	const seen = new Set<string>();
	const inspect = async (remaining: number): Promise<void> => {
		const before = seen.size,
			state = rig.drawState(),
			grid = rig.grid();
		for (const [word, color, column] of [
			['Available', good, origin],
			['Warning', 0xffa500, 0],
			['Error', error, 0],
		] as const)
			if (coloredSurfaceWord(state, grid, color, column).includes(word)) seen.add(word);
		if (seen.size !== before) await rig.capture('sections-status-' + seen.size);
		if (seen.size === 3) {
			const errorRow = gridLines(grid).findIndex((line) => line.includes('Error Sample error'));
			const start = fieldBox(state, 'action:start');
			if (errorRow >= 0)
				requireObservation(
					start.firstRow === errorRow + 2,
					'one blank row separates final section and actions',
					{ errorRow, start },
				);
			writeFileSync(
				join(rig.directory, 'sections-assertions.json'),
				JSON.stringify({ origin, statusWords: [...seen] }, null, 2),
			);
			return;
		}
		if (remaining === 0 || !(await advanceBody(rig)))
			throw new PrerequisiteGap('sample form scroll could not expose every status word and colour');
		await inspect(remaining - 1);
	};
	await inspect(128);
}
export async function observeTypography(rig: ShowcaseRig): Promise<void> {
	await rig.focus('action:close');
	const required = [
		'Body',
		'Prominent',
		'Label',
		'Rule',
		'Secondary',
		'Disabled',
		'Selected',
		'Success',
		'Warning',
		'Error',
	];
	const seen = new Set<string>();
	const inspect = async (remaining: number): Promise<void> => {
		const before = seen.size;
		for (const role of required)
			if (rig.lines().some((line) => line.includes('Public role ' + role))) seen.add(role);
		if (seen.size !== before) await rig.capture('typography-roles-' + seen.size);
		if (seen.size === required.length) {
			writeFileSync(
				join(rig.directory, 'typography-assertions.json'),
				JSON.stringify({ roles: [...seen] }, null, 2),
			);
			return;
		}
		if (remaining === 0 || !(await advanceBody(rig)))
			throw new PrerequisiteGap(
				'sample body could not expose every declared public typography role',
			);
		await inspect(remaining - 1);
	};
	await inspect(128);
}
