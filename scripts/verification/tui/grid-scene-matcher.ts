import {
	identityFailures,
	selectedAccountText,
	selectedPaceUnavailable,
} from './grid-identity-checks.ts';
/** Scene assertions over actual cells. No OCR readings, pixel reconstruction or fallback text. */
import { sceneMarkerContracts } from './grid-scene-contract.ts';
import { cellText, gridCell, gridLines } from './terminal-grid.ts';
import type { TerminalGrid } from './terminal-grid.ts';

export interface GridRectangle {
	readonly left: number;
	readonly top: number;
	readonly right: number;
	readonly bottom: number;
}
export interface GridSceneObservation {
	readonly checkpoint: string;
	readonly lines: readonly string[];
	readonly missing: readonly string[];
	readonly unexpected: readonly string[];
	readonly dialog: GridRectangle | null;
	readonly passed: boolean;
}
export const normalizeGridText = (text: string): string =>
	text.replaceAll('❯', '›').replace(/\s+/gu, ' ').trim().toLowerCase();
function rectangleLines(grid: TerminalGrid, rect: GridRectangle): readonly string[] {
	return grid.cells.slice(rect.top, rect.bottom + 1).map((row) =>
		row
			.slice(rect.left, rect.right + 1)
			.map(cellText)
			.join(''),
	);
}
export function findDialog(grid: TerminalGrid, title: string): GridRectangle | null {
	const boxes: GridRectangle[] = [];
	for (let top = 0; top < grid.rows - 2; top++) {
		for (let left = 0; left < grid.columns - 2; left++) {
			const corner = cellText(gridCell(grid, top, left));
			if (corner !== '╭' && corner !== '┌') continue;
			const right =
				grid.cells[top]?.findIndex(
					(cell, column) => column > left && ['╮', '┐'].includes(cellText(cell)),
				) ?? -1;
			if (right < 0) continue;
			for (let bottom = top + 2; bottom < grid.rows; bottom++) {
				if (
					!['╰', '└'].includes(cellText(gridCell(grid, bottom, left))) ||
					!['╯', '┘'].includes(cellText(gridCell(grid, bottom, right)))
				)
					continue;
				const rect = { left, top, right, bottom };
				if (
					rectangleLines(grid, rect)
						.slice(0, 4)
						.some((line) =>
							normalizeGridText(line.replace(/^[│┃┌╭─ ]+/u, '')).startsWith(
								normalizeGridText(title),
							),
						)
				)
					boxes.push(rect);
				break;
			}
		}
	}
	return (
		boxes.toSorted(
			(first, second) =>
				(first.right - first.left) * (first.bottom - first.top) -
				(second.right - second.left) * (second.bottom - second.top),
		)[0] ?? null
	);
}
const dialogTitles = new Set([
	'New session',
	'Session details',
	'Session filters',
	'Sessions Help',
	'Choose machine',
	'Fork session',
	'Account details',
	'Account filters',
	'Pool details',
	'Quota Help',
	'Weekly floor',
	'Options · Resets',
	'Options · Credits',
	'Credit usage',
	'Confirm reset',
	'Reset result',
]);
function fieldPair(lines: readonly string[], label: string, value: string): boolean {
	const expectedLabel = normalizeGridText(label);
	const expectedValue = normalizeGridText(value);
	return lines.some((line, row) => {
		const normalized = normalizeGridText(line.replace(/[│┃]/gu, ' '));
		if (normalized.startsWith(`${expectedLabel} `) || normalized.startsWith(`${expectedLabel}: `)) {
			const tail = normalized
				.slice(expectedLabel.length)
				.replace(/^:\s*/u, '')
				.trim()
				.replace(/^[›❯]\s*/u, '');
			return tail === expectedValue || tail === `${expectedValue} ▾`;
		}
		if (normalized !== expectedLabel && normalized !== `${expectedLabel}:`) return false;
		const next = normalizeGridText((lines[row + 1] ?? '').replace(/[│┃]/gu, ' '));
		return next === expectedValue || next === `${expectedValue} ▾`;
	});
}
export function gridFieldPair(
	grid: TerminalGrid,
	title: string,
	label: string,
	value: string,
): boolean {
	const rect = findDialog(grid, title);
	return rect !== null && fieldPair(rectangleLines(grid, rect), label, value);
}
function semanticMarker(marker: string, lines: readonly string[]): boolean | null {
	const details = /^details (session id|endpoint|state|path|title|context) (.+)$/u.exec(marker);
	if (details !== null && details[1] !== undefined && details[2] !== undefined) {
		const field = details[1];
		const value = details[2];
		if (field === 'context') {
			const context = /^(.+) (\[[^\]]+\])$/u.exec(value);
			if (context?.[1] === undefined || context[2] === undefined) return false;
			const prefix = context[1];
			const tag = context[2];
			return (
				lines.slice(0, 5).filter((line) => {
					const content = normalizeGridText(line.replace(/[│┃]/gu, ' '));
					return content.startsWith(prefix) && content.endsWith(tag);
				}).length === 1
			);
		}
		if (field === 'title') {
			const heading = lines.findIndex(
				(line) => normalizeGridText(line.replace(/[│┃]/gu, ' ')) === 'original title',
			);
			if (heading < 0) return false;
			const body = lines
				.slice(heading + 1)
				.map((line) => line.replace(/[│┃]/gu, ' '))
				.join('')
				.replace(/\s+/gu, '')
				.toLowerCase();
			return body.includes(value.replace(/\s+/gu, ''));
		}
		return fieldPair(lines, field === 'path' ? 'working directory' : field, value);
	}
	const query = /^field (.+) query (.+)$/u.exec(marker);
	if (query !== null && query[1] !== undefined && query[2] !== undefined) {
		return (
			lines.some((line) => normalizeGridText(line).includes(query[1] ?? '')) &&
			lines.some((line, row) => {
				const text = normalizeGridText(line);
				return (
					text.includes(`search: ${query[2]}▏`) ||
					text.includes(`search: ${query[2]}`) ||
					(text.replace(/[│┃]/gu, '').trim() === 'search:' &&
						normalizeGridText(lines[row + 1] ?? '').includes(`${query[2]}▏`))
				);
			})
		);
	}
	for (const label of [
		'account provider',
		'disabled accounts',
		'use credits',
		'current',
		'client',
		'eligibility',
		'freshness',
	]) {
		if (marker.startsWith(`${label} `))
			return fieldPair(lines, label, marker.slice(label.length + 1));
	}
	if (marker === 'initial reset task fact')
		return lines.some((line) =>
			/checking reset eligibility|live eligibility|reset credits|\[voucher-\d+\]/iu.test(line),
		);
	if (marker === 'initial credit task fact')
		return (
			lines.some((line) => normalizeGridText(line).includes('credit balance')) ||
			fieldPair(lines, 'use credits', 'disallow')
		);
	return null;
}
export function observeGridScene(grid: TerminalGrid, checkpoint: string): GridSceneObservation {
	const contract = sceneMarkerContracts[checkpoint];
	if (contract === undefined) throw new Error(`unknown scene contract: ${checkpoint}`);
	const lines = gridLines(grid);
	const title = contract.required.find((candidate) => dialogTitles.has(candidate));
	const dialog = title === undefined ? null : findDialog(grid, title);
	const scoped = dialog === null ? lines : rectangleLines(grid, dialog);
	const text = normalizeGridText(lines.join('\n'));
	const missing = contract.required.filter((required) => {
		const marker = normalizeGridText(required);
		if (marker === 'selected weekly pace unavailable glyph') return !selectedPaceUnavailable(grid);
		if (
			marker === 'claude research account beta full name' &&
			checkpoint.startsWith('component-quota-')
		)
			return selectedAccountText(grid) !== marker;
		const semantic = semanticMarker(marker, scoped);
		if (semantic !== null) return !semantic;
		if (/^\d+\/\d+$/u.test(marker)) return !text.split(/[^\d/]/u).includes(marker);
		if (marker.startsWith('› '))
			return !scoped.some((line) => normalizeGridText(line).includes(marker));
		return !text.includes(marker);
	});
	if (title !== undefined && dialog === null) missing.push(`active dialog ${title}`);
	missing.push(...identityFailures(grid, checkpoint, dialog));
	const unexpected = contract.forbidden.filter((marker) =>
		text.includes(normalizeGridText(marker)),
	);
	return {
		checkpoint,
		lines,
		missing,
		unexpected,
		dialog,
		passed: missing.length === 0 && unexpected.length === 0,
	};
}
