/** Inspect only the first completed native frame of the real 120x36 quota PTY. */
import assert from 'node:assert/strict';
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { z } from 'zod';

import { isMain, reportCliFailure } from './cli-arguments.ts';
import {
	cellText,
	createTerminalCore,
	gridCell,
	gridLines,
	snapshotGrid,
} from './terminal-grid.ts';

async function checkQuotaFirstFrame(): Promise<void> {
	const [evidence] = z.tuple([z.string().min(1)]).parse(process.argv.slice(2));
	const core = await createTerminalCore(120, 36);
	try {
		core.writeRaw(readFileSync(0));
		const grid = snapshotGrid(core);
		const lines = gridLines(grid);
		writeFileSync(join(evidence, 'first-frame.grid.json'), JSON.stringify(grid), {
			flag: 'wx',
			mode: 0o600,
		});
		writeFileSync(join(evidence, 'first-frame.txt'), lines.join('\n') + '\n', {
			flag: 'wx',
			mode: 0o600,
		});
		assert.equal(core.synchronizedOutput(), false, 'first frame must be completed');
		assert.equal(core.synchronizedOutputGeneration(), 1, 'later frames cannot satisfy startup');
		assert.equal(cellText(gridCell(grid, 0, 119)), '╮', 'wide top border');
		assert.equal(cellText(gridCell(grid, 35, 119)), '╯', 'full-height bottom border');
		assert(
			lines.some((line) => line.includes('Quota ·')),
			'first frame quota caption',
		);
		assert(
			lines.some((line) => line.includes('Filters')),
			'first frame wide Filters action',
		);
		assert(
			lines.some((line) => line.includes('Details')),
			'first frame wide Details action',
		);
	} finally {
		core.dispose();
	}
}

if (isMain(import.meta.url)) await checkQuotaFirstFrame().catch(reportCliFailure);
