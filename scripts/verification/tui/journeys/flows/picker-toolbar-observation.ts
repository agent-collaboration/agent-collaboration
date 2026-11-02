/** R13's approved count/filter/drop order, observed from one actual painted toolbar row. */
import { cellText, terminalTheme } from '../../terminal/terminal-grid.ts';
import type { TerminalGrid } from '../../terminal/terminal-grid.ts';
import { findDialog } from '../scenes/grid-scene-matcher.ts';

export function pickerToolbarRow(grid: TerminalGrid): {
	readonly row: number;
	readonly text: string;
} {
	const row = grid.cells.findIndex((cells) => cells.some((cell) => cellText(cell) === '≡'));
	if (row < 0) throw new Error('picker Menu glyph is not painted');
	return { row, text: (grid.cells[row] ?? []).map(cellText).join('') };
}

export function checkCurrentPickerToolbar(grid: TerminalGrid): readonly string[] {
	const { row, text } = pickerToolbarRow(grid);
	const failures: string[] = [];
	if (row !== 1 || !text.includes('Menu') || !text.includes('F10'))
		failures.push('Menu F10 left its single top row');
	const glyph = grid.cells[row]?.find((cell) => cellText(cell) === '≡');
	if (glyph?.width !== 1) failures.push('menu icon is not one terminal cell');
	const filter = text.indexOf('/');
	if (filter < 0 || (!text.includes('Filter…') && !pickerFilterFocused(grid)))
		failures.push('Filter disappeared from the toolbar');
	const count = [...text.matchAll(/(?:^|\s)(6\/6(?:\s+sessions)?|6)(?=\s|[│·]|$)/gu)].at(-1);
	if (count === undefined || count.index <= filter)
		failures.push('declared 6/6 count disappeared or moved before Filter');
	if (grid.columns === 120 && (!text.includes('6/6 sessions') || !text.includes('This machine')))
		failures.push('wide toolbar lost the full count or machine');
	if (grid.columns >= 76 && !text.includes('This machine'))
		failures.push('available machine context disappeared');
	const rightText = text.slice(1, -1).trimEnd();
	const lastStatusColumn = grid.cells[row]?.findLastIndex(
		(cell) => !/^[│┃\s]*$/u.test(cellText(cell)),
	);
	if (lastStatusColumn !== grid.columns - 3)
		failures.push('right status lost the declared outer padding alignment');
	if (
		grid.columns >= 76
			? !rightText.endsWith('This machine')
			: !/(?:6\/6(?: sessions)?|6)$/u.test(rightText)
	)
		failures.push('count/machine status is not aligned at the right edge');
	const countColumn =
		count === undefined ? -1 : count.index + count[0].length - (count[1]?.length ?? 0);
	if (grid.columns === 32 && countColumn >= 0 && countColumn - filter - 1 < 11)
		failures.push('32-column Filter has fewer than eleven outer cells');
	const scope = ['This folder', 'Repo', 'All'].some((title) => text.includes(title));
	if (!text.includes('+ New') && scope) failures.push('New dropped before the scope switch');
	if (grid.columns === 120 && !text.includes('+ New')) failures.push('wide New action disappeared');
	if (
		grid.columns === 120 &&
		!['This folder', 'Repo', 'All'].every((label) => text.includes(label))
	)
		failures.push('wide scope choices disappeared');
	if (/Help|Quit/u.test(text)) failures.push('Help or Quit appeared on the toolbar');
	return failures;
}

export function pickerListFocused(grid: TerminalGrid): boolean {
	const rectangle = findDialog(grid, 'Session');
	return (
		rectangle !== null &&
		grid.cells
			.slice(rectangle.top + 2, rectangle.bottom)
			.some((row) =>
				row
					.slice(rectangle.left + 1, rectangle.right)
					.some((cell) => cellText(cell) === '❯' && cell.fgRgb === terminalTheme.palette[11]),
			)
	);
}

export function pickerToolbarLabelFocused(grid: TerminalGrid, label: string): boolean {
	const { row, text } = pickerToolbarRow(grid);
	const column = text.indexOf(label);
	if (column < 0) return false;
	return (
		grid.cells[row]
			?.slice(column, column + label.length)
			.every((cell) => cell.fgRgb === terminalTheme.palette[11] && (cell.flags & 0x08) !== 0) ===
		true
	);
}

export function pickerVisibleFocus(grid: TerminalGrid): boolean {
	return (
		pickerListFocused(grid) ||
		grid.cells[pickerToolbarRow(grid).row]?.some(
			(cell) => cell.fgRgb === terminalTheme.palette[11] && (cell.flags & 0x08) !== 0,
		) === true
	);
}

export function pickerFilterFocused(grid: TerminalGrid): boolean {
	const { row, text } = pickerToolbarRow(grid);
	const column = text.indexOf('/');
	const boundaries = ['This folder', 'Repo', 'All']
		.map((label) => text.indexOf(label, column + 1))
		.filter((position) => position >= 0);
	const count = /\s(?:[0-9]+\/[0-9]+|[0-9]+)(?=\s|│|$)/u.exec(text.slice(column + 1));
	if (count !== null) boundaries.push(column + 1 + count.index);
	const end = boundaries.length === 0 ? text.length : Math.min(...boundaries);
	return (
		column >= 0 &&
		grid.cells[row]
			?.slice(column + 1, end)
			.some((cell) => cell.fgRgb === terminalTheme.palette[11] && (cell.flags & 0x08) !== 0) ===
			true
	);
}
