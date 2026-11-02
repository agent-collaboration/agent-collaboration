/** Exact VT observation through the owner-selected libghostty engine, without a DOM. */
import { readFileSync } from 'node:fs';

import type { CellData, CursorState, TerminalRowMetadata, TerminalThemeColors } from '@wterm/core';
import { GhosttyCore } from '@wterm/ghostty';

export interface TerminalResize {
	readonly byteOffset: number;
	readonly columns: number;
	readonly rows: number;
}

export interface TerminalGrid {
	readonly columns: number;
	readonly rows: number;
	readonly cells: readonly (readonly CellData[])[];
	readonly rowMetadata: readonly (TerminalRowMetadata | null)[];
	readonly cursor: CursorState;
}

/** Same host defaults as wterm DOM's shipped CSS; both observation routes use these. */
export const terminalTheme = {
	foreground: 0xd4d4d4,
	background: 0x1e1e1e,
	cursor: 0xaeafad,
	palette: [
		0x1e1e1e, 0xf44747, 0x6a9955, 0xd7ba7d, 0x569cd6, 0xc586c0, 0x4ec9b0, 0xd4d4d4, 0x808080,
		0xf44747, 0x6a9955, 0xd7ba7d, 0x569cd6, 0xc586c0, 0x4ec9b0, 0xffffff,
	],
} satisfies TerminalThemeColors;

export function ghosttyWasmBytes(): Buffer {
	return readFileSync(new URL(import.meta.resolve('@wterm/ghostty/ghostty-vt.wasm')));
}

export async function createTerminalCore(columns: number, rows: number): Promise<GhosttyCore> {
	// The public loader uses fetch. A data URL loads the exact pinned asset in
	// Node without a file-fetch shim, a server, or mutation of global fetch.
	const core = await GhosttyCore.load({
		wasmPath: `data:application/wasm;base64,${ghosttyWasmBytes().toString('base64')}`,
		imageStorageLimit: 0,
	});
	core.init(columns, rows);
	core.setThemeColors(terminalTheme);
	if (core.getCols() !== columns || core.getRows() !== rows) {
		core.dispose();
		throw new Error('terminal core did not apply the requested dimensions');
	}
	return core;
}

export function snapshotGrid(core: GhosttyCore): TerminalGrid {
	const columns = core.getCols();
	const rows = core.getRows();
	return {
		columns,
		rows,
		cells: Array.from({ length: rows }, (_rowValue, row) =>
			Array.from({ length: columns }, (_cellValue, col) => ({ ...core.getCell(row, col) })),
		),
		rowMetadata: Array.from({ length: rows }, (_, row) => core.getRowMetadata(row)),
		cursor: { ...core.getCursor() },
	};
}

export function gridCell(grid: TerminalGrid, row: number, column: number): CellData {
	const cell = grid.cells[row]?.[column];
	if (cell === undefined) throw new Error(`cell outside grid: ${column},${row}`);
	return cell;
}

export function cellText(cell: CellData): string {
	if (cell.width === 0 || cell.spacerHead === true) return '';
	return cell.chars ?? String.fromCodePoint(cell.char);
}

export function gridLines(grid: TerminalGrid): readonly string[] {
	return grid.cells.map((row) => row.map(cellText).join(''));
}

/** Replay PTY bytes with the actual kernel resize boundaries, in order. */
export function replayTerminal(
	core: GhosttyCore,
	bytes: Uint8Array,
	resizes: readonly TerminalResize[],
): void {
	let offset = 0;
	for (const resize of resizes) {
		if (resize.byteOffset < offset || resize.byteOffset > bytes.length)
			throw new Error('invalid terminal resize boundary');
		core.writeRaw(bytes.subarray(offset, resize.byteOffset));
		core.resize(resize.columns, resize.rows);
		offset = resize.byteOffset;
	}
	core.writeRaw(bytes.subarray(offset));
}
