/** Identity/selection checks use cell positions and styles, never a matching word elsewhere. */
import { cellText, gridLines, terminalTheme } from '../../terminal/terminal-grid.ts';
import type { TerminalGrid } from '../../terminal/terminal-grid.ts';
import type { GridRectangle } from './grid-scene-matcher.ts';
const normalize = (text: string): string => text.replace(/\s+/gu, ' ').trim().toLowerCase();
const compactCensus = {
	accounts: 4,
	usable: 0,
	reserve: 0,
	blocked: 0,
	'pool unknown': 1,
	excluded: 3,
	fresh: 0,
	stale: 0,
	'data unknown': 4,
	clients: 0,
};
const hardCensus = {
	accounts: 2,
	usable: 1,
	reserve: 0,
	blocked: 1,
	'pool unknown': 0,
	excluded: 0,
	fresh: 0,
	stale: 1,
	'data unknown': 1,
	clients: 1,
};
function activeLines(grid: TerminalGrid, dialog: GridRectangle | null): readonly string[] {
	return dialog === null
		? gridLines(grid)
		: grid.cells.slice(dialog.top + 1, dialog.bottom).map((row) =>
				row
					.slice(dialog.left + 1, dialog.right)
					.map(cellText)
					.join(''),
			);
}
export function selectedAccountText(grid: TerminalGrid): string | null {
	const choices = grid.cells
		.flatMap((row, rowIndex) => row.map((cell, col) => ({ cell, col, rowIndex })))
		.filter(
			({ cell }) => ['❯', '›'].includes(cellText(cell)) && cell.fgRgb === terminalTheme.palette[11],
		);
	if (choices.length !== 1) return null;
	const marker = choices[0];
	if (marker === undefined) return null;
	const text: string[] = [];
	for (let row = marker.rowIndex; row < Math.min(marker.rowIndex + 4, grid.rows); row++) {
		let part = '';
		for (const cell of grid.cells[row]?.slice(marker.col + 1) ?? []) {
			if (cell.fgRgb === terminalTheme.palette[11]) part += cellText(cell);
			else if (cellText(cell).trim() === '') part += ' ';
			else break;
		}
		if (part.trim() === '') break;
		text.push(part);
	}
	return normalize(text.join(' '));
}
export function selectedPaceUnavailable(grid: TerminalGrid): boolean {
	const cells = grid.cells.flatMap((row, rowIndex) =>
		row.map((cell, column) => ({ cell, rowIndex, column })),
	);
	const marker = cells.find(
		({ cell }) => ['❯', '›'].includes(cellText(cell)) && cell.fgRgb === terminalTheme.palette[11],
	);
	// UiSurface::Selected in the adopted shipped theme. This also binds a
	// scrolled folded group whose account name has left the viewport.
	const selectedBackground = marker?.cell.bgRgb ?? 0x343b46;
	const glyphs = cells.filter(
		({ cell }) => cellText(cell) === '⏳' && cell.bgRgb === selectedBackground,
	);
	if (glyphs.length !== 1) return false;
	const glyph = glyphs[0];
	if (glyph === undefined || glyph.cell.width !== 2) return false;
	if (grid.cells[glyph.rowIndex]?.[glyph.column + 1]?.width !== 0) return false;
	const header = gridLines(grid).findIndex((line) => line.includes('Weekly pace'));
	if (header < 0) return false;
	const headerColumn = gridLines(grid)[header]?.indexOf('Weekly pace') ?? -1;
	return (
		glyph.column === headerColumn ||
		(header < glyph.rowIndex &&
			glyph.rowIndex - header <= 3 &&
			grid.cells[header]?.[headerColumn]?.bgRgb === selectedBackground)
	);
}
const selectedBetaScenes = new Set([
	'component-quota-next-account',
	'component-quota-hard-beta-windows',
	'component-quota-hard-beta-extra',
	'component-quota-hard-pool-closed',
	'component-quota-hard-beta-selected',
	'component-quota-hard-pool-closed-top',
]);
const selectedAlphaScenes = new Set([
	'component-quota-restored-account',
	'component-quota-browse-page-returned',
	'component-quota-hard-browse',
]);
const windowRows: Readonly<Record<string, readonly string[]>> = {
	'component-quota-hard-browse': ['92% · week · 6d 16h', '83% · 5h · 2h 10m'],
	'component-quota-hard-beta-windows': ['75% · week'],
	'component-quota-hard-alpha-week': ['92% · week · 6d 16h'],
	'component-quota-hard-alpha-short': ['83% · 5h · 2h 10m'],
	'component-quota-hard-beta-week': ['75% · week'],
};
export function identityFailures(
	grid: TerminalGrid,
	checkpoint: string,
	dialog: GridRectangle | null,
): readonly string[] {
	const failures: string[] = [];
	const scoped = activeLines(grid, dialog).map(normalize);
	if (
		[
			'component-quota-browse-credit-window',
			'component-quota-browse-pace-heading',
			'component-quota-browse-pace-value',
		].includes(checkpoint)
	) {
		const selected = selectedAccountText(grid);
		if (selected !== null && selected !== '東京 openai research account alpha long name')
			failures.push('Alpha pose belongs to selected Alpha, not a visible unselected fact');
		const markers = grid.cells
			.flatMap((row) => row)
			.filter((cell) => ['❯', '›'].includes(cellText(cell)));
		if (selected === null && markers.length !== 0)
			failures.push('visible selected identity is ambiguous');
	}
	if (checkpoint.includes('pool-details')) {
		const census = checkpoint.includes('hard-') ? hardCensus : compactCensus;
		const keys = checkpoint.endsWith('-complete')
			? Object.keys(census)
			: checkpoint.endsWith('-middle')
				? [
						'blocked',
						'pool unknown',
						'excluded',
						...(checkpoint.includes('hard-') ? ['fresh'] : []),
					]
				: checkpoint.endsWith('-bottom')
					? [...(checkpoint.includes('hard-') ? [] : ['fresh']), 'stale', 'data unknown', 'clients']
					: ['accounts', 'usable', 'reserve'];
		for (const label of keys) {
			const count = Object.entries(census).find(([name]) => name === label)?.[1];
			if (!scoped.some((line) => line === `${label} ${count}` || line === `${label}: ${count}`))
				failures.push(`Pool field ${label} ${count}`);
		}
		if (
			checkpoint.includes('hard-') &&
			(checkpoint.endsWith('-bottom') || checkpoint.endsWith('-complete')) &&
			!scoped.some((line) => line.includes('credentials locked'))
		)
			failures.push('Pool credential notice');
	}
	if (selectedBetaScenes.has(checkpoint) || selectedAlphaScenes.has(checkpoint)) {
		const expected = selectedBetaScenes.has(checkpoint)
			? 'claude research account beta full name'
			: '東京 openai research account alpha long name';
		const actual = selectedAccountText(grid);
		const short = checkpoint.endsWith('-selected') || checkpoint.endsWith('-closed-top');
		if (actual !== (short ? expected.replace(/ name$/u, '') : expected))
			failures.push(`selected account ${expected}`);
	}
	for (const row of windowRows[checkpoint] ?? [])
		if (!scoped.some((line) => line.includes(row))) failures.push(`one-row quota window ${row}`);
	if (checkpoint === 'component-quota-hard-beta-windows') {
		const selectedRows = grid.cells
			.filter((row) => row.some((cell) => cell.bgRgb === 0x343b46))
			.map((row) =>
				normalize(
					row
						.filter((cell) => cell.bgRgb === 0x343b46)
						.map(cellText)
						.join(''),
				),
			);
		if (
			!selectedRows.some((row) => row.includes('75% · week')) ||
			!selectedRows.some((row) => row.includes('week stale'))
		)
			failures.push('Beta selected Quota cell amount and stale warning');
		if (selectedRows.some((row) => row.includes('· —') || row.includes('no data')))
			failures.push('compact Beta browse omits unknown-window detail');
	}
	if (checkpoint === 'component-quota-hard-details') {
		const weekly = scoped.find((row) => row.startsWith('weekly '));
		const short = scoped.find((row) => row.startsWith('5h '));
		if (
			weekly === undefined ||
			!weekly.includes('75%') ||
			!weekly.includes('reset unknown') ||
			weekly.includes('7d')
		)
			failures.push('Beta Details weekly75% reset unknown');
		if (short === undefined || !short.includes('no data') || short.includes('%'))
			failures.push('Beta Details missing5h no data and no percent');
	}
	if (
		checkpoint === 'component-quota-credits-refreshed-observation' &&
		!scoped.some((line) => /^observation:? checked 0s ago$/u.test(line))
	)
		failures.push('labelled zero-age observation');
	if (
		checkpoint === 'component-quota-credits-refreshed-balance' &&
		!scoped.some((line) => /(?<![\d,.])63,000(?![\d,.])/u.test(line))
	)
		failures.push('exact refreshed balance');
	if (
		checkpoint.includes('confirm-') &&
		!checkpoint.includes('help-') &&
		!checkpoint.endsWith('returned')
	) {
		const expected = checkpoint.endsWith('-yes') ? 'Yes' : 'No';
		const words = grid.cells
			.flatMap((row) =>
				row.filter((cell) => cell.fgRgb === terminalTheme.palette[11]).map(cellText),
			)
			.join('');
		if (!words.includes(expected)) failures.push(`selected decision ${expected}`);
	}
	if (checkpoint === 'component-quota-policy-choice') {
		const selected = scoped.filter((line) => /[❯›]\s*allow\b/u.test(line));
		if (selected.length !== 1) failures.push('exactly one highlighted Allow in active choice');
	}
	if (
		checkpoint === 'compact-sessions-details' ||
		checkpoint.startsWith('compact-sessions-details-')
	) {
		if (scoped.some((line) => /enter\s+(?:to\s+)?resume/u.test(line)))
			failures.push('fixed Enter-is-Resume hint');
	}
	return failures;
}
