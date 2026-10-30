/** Controlled physical cells qualify the reader, not the sample component. */
import { expect, test } from 'vitest';

import { createTerminalCore, snapshotGrid, terminalTheme } from '../terminal-grid.ts';
import type { ShowcaseState } from './contracts.ts';
import { coloredSurfaceWord, inspectSectionGeometry } from './form-observations.ts';

test('wrapped field heights and status words keep physical origins and exact gaps', async () => {
	const core = await createTerminalCore(80, 20);
	try {
		core.writeString(
			'\x1b[4;3HDraft connection\x1b[5;3HDirectory\x1b[5;30Hoject\x1b[6;3HOption\x1b[6;30HShar\x1b[7;30Hed la\x1b[8;30Hbel\x1b[10;3HTinted status\x1b[11;3HLong section label\x1b[11;30H\x1b[32mAvai\x1b[12;30Hlabl\x1b[13;30He\x1b[0m\x1b[11;36HLocal detail',
		);
		const state = {
			surface: 'overlay:1',
			hitMap: {
				frame: 1,
				surfaces: [
					{
						surface: 'overlay:1',
						cells: [
							[1, 1],
							[62, 16],
						],
					},
				],
				targets: [
					{
						surface: 'overlay:1',
						target: 'draft',
						kind: 'target',
						cells: [
							[29, 4],
							[34, 4],
						],
					},
					{
						surface: 'overlay:1',
						target: 'choice',
						kind: 'target',
						cells: [
							[29, 5],
							[35, 7],
						],
					},
				],
			},
		} satisfies Pick<ShowcaseState, 'hitMap' | 'surface'>;
		const grid = snapshotGrid(core);
		expect(inspectSectionGeometry(state, grid)).toBe(29);
		const good = terminalTheme.palette[2];
		if (good === undefined) throw new Error('green status palette absent');
		expect(coloredSurfaceWord(state, grid, good, 29)).toBe('Available');
		const shifted = {
			...state,
			hitMap: {
				...state.hitMap,
				targets: state.hitMap.targets.map((target) =>
					target.target === 'choice'
						? {
								...target,
								cells: [
									[30, 5],
									[35, 7],
								] satisfies [number, number][],
							}
						: target,
				),
			},
		};
		expect(() => inspectSectionGeometry(shifted, grid)).toThrow('value origin');
		const missing = { ...state, hitMap: { ...state.hitMap, targets: [] } };
		expect(() => inspectSectionGeometry(missing, grid)).toThrow('exposes no field');
		core.writeString('\x1b[10;3H             \x1b[9;3HTinted status');
		expect(() => inspectSectionGeometry(state, snapshotGrid(core))).toThrow();
	} finally {
		core.dispose();
	}
});
