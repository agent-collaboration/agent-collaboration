/** Literal R13 quota chrome and Help census, independent of native receipt fields. */
import { cellText, terminalTheme } from '../../terminal/terminal-grid.ts';
import type { TerminalGrid } from '../../terminal/terminal-grid.ts';

export const quotaToolbarLabels = ['Details', 'Filters:', 'Pool', 'Options', 'Floor'] as const;
export function quotaToolbarRow(grid: TerminalGrid): {
	readonly row: number;
	readonly text: string;
} {
	const row = grid.cells.findIndex((cells) => cells.some((cell) => cellText(cell) === '≡'));
	if (row < 0) throw new Error('quota Menu glyph is not painted');
	return { row, text: (grid.cells[row] ?? []).map(cellText).join('') };
}
export function checkQuotaToolbar(grid: TerminalGrid): readonly string[] {
	const { row, text } = quotaToolbarRow(grid),
		failures: string[] = [];
	if (row !== 1 || !text.includes('Menu') || !text.includes('F10'))
		failures.push('Menu F10 left the single top row');
	if (grid.cells[row]?.find((cell) => cellText(cell) === '≡')?.width !== 1)
		failures.push('quota Menu glyph is not one cell');
	if (!text.includes('Quota · 2/2')) failures.push('declared quota account counts disappeared');
	if (grid.columns === 120 && !text.includes('Quota · 2/2 · 1 client'))
		failures.push('wide client count disappeared');
	const visible = quotaToolbarLabels.filter((label) => text.includes(label));
	if (JSON.stringify(visible) !== JSON.stringify(quotaToolbarLabels.slice(0, visible.length)))
		failures.push('quota toolbar did not drop its right suffix');
	if (
		visible.some(
			(label, index) => index > 0 && text.indexOf(label) < text.indexOf(visible[index - 1] ?? ''),
		)
	)
		failures.push('quota toolbar action order differs');
	if (grid.columns === 120 && visible.length !== quotaToolbarLabels.length)
		failures.push('wide quota toolbar omitted an action');
	if (/Help|Quit/u.test(text)) failures.push('Help or Quit appeared on quota toolbar');
	return failures;
}
export function quotaToolbarFocus(grid: TerminalGrid, label: string): boolean {
	const { row } = quotaToolbarRow(grid),
		cells = grid.cells[row] ?? [],
		text = cells.map(cellText).join(''),
		column = text.indexOf(label);
	return (
		column >= 0 &&
		cells
			.slice(column, column + label.length)
			.every((cell) => cell.fgRgb === terminalTheme.palette[11] && (cell.flags & 0x08) !== 0)
	);
}
export function quotaListFocused(grid: TerminalGrid): boolean {
	return grid.cells.some((row) =>
		row.some((cell) => cellText(cell) === '東' && cell.fgRgb === terminalTheme.palette[11]),
	);
}
export const quotaHelpActions = [
	'F10 Menu',
	'F4, Enter Details',
	'F3, Ctrl+F Filters…',
	'F5 Pool',
	'Ctrl+R Options',
	'Ctrl+E Floor',
	'Log in again',
	'R Refresh',
	'F6 Group by',
	'None',
	'Provider',
	'Status',
	'F1, Ctrl+7, Ctrl+_, Ctrl+/ Help',
	'Q, Ctrl+C, Ctrl+D Quit',
] as const;
const conventions = [
	'↑ / ↓ Select an account',
	'Tab/Shift+Tab Move between controls',
	'Esc Close the top menu or dialog; stays on quota',
	'Ctrl+C Exit Quota unless a reset is being applied.',
] as const;
const compact = (text: string): string => text.replace(/[\s│█]/gu, '');
export function checkQuotaHelpRows(rows: readonly string[]): readonly string[] {
	const text = compact(rows.join(' ')),
		start = text.indexOf('Actions'),
		end = text.indexOf('Inputconventions');
	if (start < 0 || end <= start) return ['quota Help action/convention sections absent'];
	const failures: string[] = [];
	if (text.slice(start + 'Actions'.length, end) !== compact(quotaHelpActions.join(' ')))
		failures.push('quota Help closed action/key census differs');
	const body = text.slice(end + 'Inputconventions'.length),
		expected = compact(conventions.join(' '));
	if (!body.startsWith(expected)) failures.push('quota Help input conventions differ');
	return failures;
}
