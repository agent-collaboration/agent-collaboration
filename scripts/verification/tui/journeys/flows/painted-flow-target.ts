/** Resolve an unambiguous, fully painted label in cell coordinates, including wide glyphs. */
import { cellText } from '../../terminal/terminal-grid.ts';
import type { TerminalGrid } from '../../terminal/terminal-grid.ts';

export interface PaintedTargetBounds {
	readonly left: number;
	readonly top: number;
	readonly right: number;
	readonly bottom: number;
}
export interface PaintedFlowTarget {
	readonly label: string;
	/** The active surface's visible interior, never an underlying matching label. */
	readonly bounds: PaintedTargetBounds;
	readonly foregrounds?: readonly number[];
}
export interface PaintedTargetLocation {
	readonly column: number;
	readonly row: number;
	readonly width: number;
}
export function locatePaintedTarget(
	grid: TerminalGrid,
	target: PaintedFlowTarget,
): PaintedTargetLocation {
	const { left, top, right, bottom } = target.bounds;
	if (
		!target.label.trim() ||
		![left, top, right, bottom].every(Number.isSafeInteger) ||
		left < 0 ||
		top < 0 ||
		right > grid.columns ||
		bottom > grid.rows ||
		left >= right ||
		top >= bottom
	)
		throw new Error('painted target requires a nonempty label and valid visible bounds');
	const found: PaintedTargetLocation[] = [];
	for (let row = top; row < bottom; row += 1) {
		const cells = grid.cells[row];
		if (cells === undefined) throw new Error('painted grid row absent');
		const starts = new Map<number, number>(),
			ends = new Map<number, number>();
		let text = '';
		for (let column = left; column < right; column += 1) {
			const cell = cells[column];
			if (cell === undefined) throw new Error('painted grid cell absent');
			const value = cellText(cell);
			if (!value) continue;
			// A partially clipped wide character cannot supply a click target.
			const width = cell.width ?? 1;
			if (column + width > right) continue;
			starts.set(text.length, column);
			text += value;
			ends.set(text.length, column + width);
		}
		for (
			let offset = text.indexOf(target.label);
			offset >= 0;
			offset = text.indexOf(target.label, offset + 1)
		) {
			const column = starts.get(offset),
				end = ends.get(offset + target.label.length);
			if (
				column !== undefined &&
				end !== undefined &&
				(target.foregrounds === undefined ||
					cells
						.slice(column, end)
						.every((cell) => cell.fgRgb !== undefined && target.foregrounds?.includes(cell.fgRgb)))
			)
				found.push({ column, row, width: end - column });
		}
	}
	if (found.length !== 1)
		throw new Error(
			`painted target ${JSON.stringify(target.label)} has ${found.length} visible matches`,
		);
	const location = found[0];
	if (location === undefined) throw new Error('painted target location absent');
	return location;
}
