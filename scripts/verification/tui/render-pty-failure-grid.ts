/** Retain the actual last libghostty grid from an opt-in failed PTY wait. */
import { readFileSync, writeFileSync } from 'node:fs';

import { z } from 'zod';

import { isMain, reportCliFailure } from './cli-arguments.ts';
import { createTerminalCore, gridLines, replayTerminal, snapshotGrid } from './terminal-grid.ts';

const geometrySchema = z.strictObject({
	initialColumns: z.number().int().positive(),
	initialRows: z.number().int().positive(),
	resizes: z.array(
		z.strictObject({
			byteOffset: z.number().int().nonnegative(),
			columns: z.number().int().positive(),
			rows: z.number().int().positive(),
		}),
	),
});

async function renderPtyFailureGrid(): Promise<void> {
	const [prefix] = z.tuple([z.string().min(1)]).parse(process.argv.slice(2));
	const geometry = geometrySchema.parse(JSON.parse(readFileSync(`${prefix}.json`, 'utf8')));
	const bytes = readFileSync(`${prefix}.ansi`);
	const core = await createTerminalCore(geometry.initialColumns, geometry.initialRows);
	try {
		replayTerminal(core, bytes, geometry.resizes);
		const grid = snapshotGrid(core);
		writeFileSync(`${prefix}.grid.json`, JSON.stringify(grid), { flag: 'wx', mode: 0o600 });
		writeFileSync(`${prefix}.txt`, `${gridLines(grid).join('\n')}\n`, { flag: 'wx', mode: 0o600 });
	} finally {
		core.dispose();
	}
}

if (isMain(import.meta.url)) await renderPtyFailureGrid().catch(reportCliFailure);
