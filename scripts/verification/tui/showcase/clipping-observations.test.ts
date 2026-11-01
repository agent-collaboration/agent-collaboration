/** Deliberately wrong published cells qualify the observer, not the component. */
import { expect, test } from 'vitest';

import { createTerminalCore, snapshotGrid } from '../terminal-grid.ts';
import { assertClippingPose } from './clipping-observations.ts';
import type { ClippingFixture } from './clipping-observations.ts';
import type { ShowcaseState } from './contracts.ts';

const fixtures = [
	['plain-horizontal', 3, 0],
	['plain-vertical', 9, 1],
	['bordered-horizontal', 4, 0],
	['bordered-vertical', 9, 1],
	['nested-horizontal', 3, 0],
	['nested-vertical', 6, 1],
] satisfies readonly (readonly [ClippingFixture, number, number])[];

test.each(fixtures)(
	'%s rejects shifted, unbounded, hidden and unpainted cells',
	async (fixture, width, rowOffset) => {
		const core = await createTerminalCore(40, 12);
		try {
			const partialColumn = rowOffset === 1 ? 3 : 9;
			core.writeString(`\x1b[4;5HH\x1b[${4 + rowOffset};${partialColumn + 2}HP`);
			const state = {
				surface: 'overlay:1',
				focusTargets: [{ targetId: 'action:clip-hidden', role: 'pointerOnly', eligible: true }],
				hitMap: {
					frame: 1,
					surfaces: [],
					targets: [
						{
							surface: 'overlay:1',
							target: 'action:clip-hit',
							kind: 'target',
							cells: Array.from({ length: 5 }, (_, column): [number, number] => [column + 3, 3]),
						},
						{
							surface: 'overlay:1',
							target: 'action:clip-partial',
							kind: 'target',
							cells: Array.from({ length: width }, (_, column): [number, number] => [
								column + partialColumn,
								3 + rowOffset,
							]),
						},
					],
				},
			} satisfies Pick<ShowcaseState, 'hitMap' | 'surface' | 'focusTargets'>;
			const good = snapshotGrid(core);
			expect(assertClippingPose(state, good, fixture).origin).toEqual({ column: 3, row: 3 });
			for (const mutate of [
				(cells: readonly [number, number][]): [number, number][] =>
					cells.map(([column, row]) => [column + 1, row]),
				(cells: readonly [number, number][]): [number, number][] => [
					...cells,
					[partialColumn + width, 3 + rowOffset],
				],
				(cells: readonly [number, number][]): [number, number][] => [
					...cells,
					[partialColumn, 4 + rowOffset],
				],
			]) {
				const wrong = {
					...state,
					hitMap: {
						...state.hitMap,
						targets: state.hitMap.targets.map((target) =>
							target.target === 'action:clip-partial'
								? { ...target, cells: mutate(target.cells) }
								: target,
						),
					},
				};
				expect(() => assertClippingPose(wrong, good, fixture)).toThrow('clip intersection');
			}
			const hidden = {
				...state,
				hitMap: {
					...state.hitMap,
					targets: [
						...state.hitMap.targets,
						{
							surface: 'overlay:1',
							target: 'action:clip-hidden',
							kind: 'target',
							cells: [[24, 3]],
						},
					],
				},
			} satisfies Pick<ShowcaseState, 'hitMap' | 'surface' | 'focusTargets'>;
			expect(() => assertClippingPose(hidden, good, fixture)).toThrow('no visible cells');
			const missing = { ...state, hitMap: { ...state.hitMap, targets: [] } };
			expect(() => assertClippingPose(missing, good, fixture)).toThrow(
				'positive clipping controls',
			);
			core.writeString('\x1b[2J');
			expect(() => assertClippingPose(state, snapshotGrid(core), fixture)).toThrow(
				'really painted',
			);
		} finally {
			core.dispose();
		}
	},
);
