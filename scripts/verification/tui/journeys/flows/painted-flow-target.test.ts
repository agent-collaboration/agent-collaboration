import { expect, test } from 'vitest';

import { createTerminalCore, snapshotGrid } from '../../terminal/terminal-grid.ts';
import { locatePaintedTarget } from './painted-flow-target.ts';

test('visible surface bounds disambiguate repeated labels and wide glyphs preserve cell columns', async () => {
	const core = await createTerminalCore(32, 12);
	try {
		core.writeString('界 Start\x1b[4;1H📂 e\u0301 Start');
		const grid = snapshotGrid(core);
		expect(
			locatePaintedTarget(grid, {
				label: 'Start',
				bounds: { left: 0, top: 3, right: 32, bottom: 5 },
			}),
		).toEqual({ column: 5, row: 3, width: 5 });
		expect(() =>
			locatePaintedTarget(grid, {
				label: 'Start',
				bounds: { left: 0, top: 0, right: 32, bottom: 12 },
			}),
		).toThrow('2 visible matches');
	} finally {
		core.dispose();
	}
});
test('clipped, hidden and half-glyph labels cannot be activated', async () => {
	const core = await createTerminalCore(32, 12);
	try {
		core.writeString('界 Start');
		const grid = snapshotGrid(core);
		for (const [label, bounds] of [
			['Start', { left: 0, top: 0, right: 7, bottom: 1 }],
			['Start', { left: 0, top: 1, right: 32, bottom: 2 }],
			['界', { left: 1, top: 0, right: 2, bottom: 1 }],
			['界', { left: 0, top: 0, right: 1, bottom: 1 }],
		] as const)
			expect(() => locatePaintedTarget(grid, { label, bounds })).toThrow('0 visible matches');
	} finally {
		core.dispose();
	}
});
