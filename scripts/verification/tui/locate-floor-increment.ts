/** Read a real PTY frame with the qualified observer; never infer a click from layout constants. */
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { z } from 'zod';

import { isMain, reportCliFailure } from './cli-arguments.ts';
import { findDialog } from './grid-scene-matcher.ts';
import { cellText, createTerminalCore, snapshotGrid } from './terminal-grid.ts';

interface ControlCell {
	readonly column: number;
	readonly row: number;
}

async function locateFloorIncrement(): Promise<void> {
	const [columns, rows, evidence] = z
		.tuple([
			z.coerce.number().int().positive().max(1000),
			z.coerce.number().int().positive().max(1000),
			z.string().min(1),
		])
		.parse(process.argv.slice(2));
	const bytes = readFileSync(0);
	const core = await createTerminalCore(columns, rows);
	try {
		core.writeRaw(bytes);
		const grid = snapshotGrid(core);
		writeFileSync(join(evidence, 'before-click.ansi'), bytes, { flag: 'wx', mode: 0o600 });
		writeFileSync(join(evidence, 'before-click.grid.json'), JSON.stringify(grid), {
			flag: 'wx',
			mode: 0o600,
		});
		const dialog = findDialog(grid, 'Weekly floor');
		if (dialog === null) throw new Error('Weekly floor is absent from the actual PTY grid');
		const matches: ControlCell[] = [];
		for (let row = dialog.top + 1; row < dialog.bottom; row++) {
			for (let column = dialog.left + 1; column < dialog.right; column++) {
				const cell = grid.cells[row]?.[column];
				if (cell !== undefined && cellText(cell) === '▶') matches.push({ column, row });
			}
		}
		if (matches.length !== 1) throw new Error('Weekly floor requires exactly one increment cell');
		process.stdout.write(JSON.stringify(matches[0]) + '\n');
	} finally {
		core.dispose();
	}
}

if (isMain(import.meta.url)) await locateFloorIncrement().catch(reportCliFailure);
