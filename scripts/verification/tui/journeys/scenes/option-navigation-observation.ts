import { cellText, gridLines } from '../../terminal/terminal-grid.ts';
import type { TerminalGrid } from '../../terminal/terminal-grid.ts';
/** Physical viewport and stable-label observations, independent of the app's paging counters. */
import { findDialog } from './grid-scene-matcher.ts';
import type { GridRectangle } from './grid-scene-matcher.ts';

export interface SessionNavigationRow {
	readonly label: string;
	readonly visiblePrefix: string;
}
export function selectedFixtureRow(
	grid: TerminalGrid,
	records: readonly SessionNavigationRow[],
): number {
	const rectangle = findDialog(grid, 'Session');
	if (rectangle === null) throw new Error('picker list frame missing');
	const lines = gridLines(grid).slice(rectangle.top + 2, rectangle.bottom);
	// Focused ListRow uses UiGlyph::Focus (theme.rs), distinct from Current.
	const selected = lines.filter((line) => line.includes('❯'));
	const matches = records.flatMap((record, index) =>
		selected.some((line) => line.includes(record.visiblePrefix)) ? [index] : [],
	);
	if (matches.length !== 1 || matches[0] === undefined)
		throw new Error('selected fixture identity missing or ambiguous');
	return matches[0];
}
export function pickerPageRows(grid: TerminalGrid): number {
	const rectangle = findDialog(grid, 'Session');
	if (rectangle === null) throw new Error('picker list frame missing');
	// A list frame has two border rows and one column-heading row. Session
	// content has title+metadata+gap, plus a status row below forty columns.
	const viewportRows = rectangle.bottom - rectangle.top - 2;
	if (viewportRows <= 0) throw new Error('picker list has no physical body rows');
	return Math.max(1, Math.floor(viewportRows / (grid.columns < 40 ? 4 : 3)));
}
export interface HelpViewport {
	readonly rectangle: GridRectangle;
	readonly rows: readonly string[];
	readonly atEnd: boolean;
}
function trackedHelpViewport(grid: TerminalGrid): HelpViewport | null {
	const rectangle = findDialog(grid, 'Help');
	if (rectangle === null) throw new Error('Help frame missing');
	// The focused public ScrollList paints a thumb and a contiguous track.
	// Derive the actual body interval from those cells, excluding frame/footer.
	let track: { column: number; rows: number[] } | null = null;
	for (let column = rectangle.right - 1; column > rectangle.left; column -= 1) {
		const rows: number[] = [];
		let thumb = false;
		for (let row = rectangle.top + 1; row < rectangle.bottom; row += 1) {
			const cell = grid.cells[row]?.[column];
			const value = cell === undefined ? '' : cellText(cell);
			if (value === '█' || value === '│') {
				rows.push(row);
				if (value === '█') thumb = true;
			}
		}
		if (
			thumb &&
			rows.length > 1 &&
			rows.every((row, index) => index === 0 || row === (rows[index - 1] ?? -2) + 1)
		) {
			track = { column, rows };
			break;
		}
	}
	if (track === null) return null;
	const rows = track.rows.map((row) =>
		(grid.cells[row] ?? [])
			.slice(rectangle.left + 1, track.column)
			.map(cellText)
			.join('')
			.trimEnd(),
	);
	const last = track.rows.at(-1);
	const lastCell = last === undefined ? undefined : grid.cells[last]?.[track.column];
	return { rectangle, rows, atEnd: lastCell !== undefined && cellText(lastCell) === '█' };
}
export function helpViewport(grid: TerminalGrid): HelpViewport {
	const viewport = trackedHelpViewport(grid);
	if (viewport === null) throw new Error('Help scroll track is not independently observable');
	return viewport;
}
const compactHelpText = (text: string): string => text.replace(/\s/gu, '');
export function requireFullHelpCensus(rows: readonly string[], census: readonly string[]): void {
	if (census.length === 0) throw new Error('independent full Help census required');
	const actual = compactHelpText(rows.join(''));
	const expected = compactHelpText(census.join(''));
	if (actual !== expected) {
		const missing = census.filter((row) => !actual.includes(compactHelpText(row)));
		throw new Error(
			'full Help census not visible: ' +
				(missing.length > 0 ? missing.join(' | ') : 'row order or unexpected content differs'),
		);
	}
}
export type HelpLayout =
	| { readonly kind: 'overflow'; readonly viewport: HelpViewport }
	| { readonly kind: 'fit'; readonly rectangle: GridRectangle; readonly rows: readonly string[] }
	| {
			readonly kind: 'incomplete';
			readonly rectangle: GridRectangle;
			readonly rows: readonly string[];
			readonly reason: string;
	  };
export function helpLayout(grid: TerminalGrid, census: readonly string[]): HelpLayout {
	const tracked = trackedHelpViewport(grid);
	if (tracked !== null) return { kind: 'overflow', viewport: tracked };
	const rectangle = findDialog(grid, 'Help');
	if (rectangle === null) throw new Error('Help frame missing');
	const lines = grid.cells.slice(rectangle.top + 2, rectangle.bottom).map((row) =>
		row
			.slice(rectangle.left + 1, rectangle.right)
			.map(cellText)
			.join('')
			.trimEnd(),
	);
	const footer = lines.findLastIndex((line) => line.includes('Close Help'));
	if (footer < 0) throw new Error('Help footer missing');
	const rows = lines.slice(0, footer);
	try {
		requireFullHelpCensus(rows, census);
		return { kind: 'fit', rectangle, rows };
	} catch (error) {
		return { kind: 'incomplete', rectangle, rows, reason: String(error) };
	}
}
export function requireHelpNoop(before: TerminalGrid, after: TerminalGrid): void {
	if (JSON.stringify(before) !== JSON.stringify(after))
		throw new Error('fitting Help alias changed its marked frame');
}
export function appendOneHelpRow(previous: readonly string[], next: readonly string[]): string {
	if (
		previous.length !== next.length ||
		previous.length === 0 ||
		previous.slice(1).some((row, index) => row !== next[index])
	)
		throw new Error('plain Down did not move the physical Help body by exactly one row');
	const added = next.at(-1);
	if (added === undefined) throw new Error('Help body empty');
	return added;
}
export function requireHelpRows(actual: readonly string[], expected: readonly string[]): void {
	if (JSON.stringify(actual) !== JSON.stringify(expected))
		throw new Error('Help alias did not reach its independently indexed physical page');
}
