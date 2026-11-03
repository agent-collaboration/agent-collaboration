import { expect, test } from 'vitest';

import { showcasePlanSchema, scenarioIds } from './contracts.ts';
import type { ShowcasePlan } from './contracts.ts';
import { scenarioMatrix } from './scenario-matrix.ts';
import { variantLabel } from './scenario-variants.ts';
function plan(): ShowcasePlan {
	return showcasePlanSchema.parse({
		kind: 'component-showcase',
		repository: '/declared',
		exampleSource: {
			repository: '/example-checkout',
			source: { head: 'f'.repeat(40), files: { 'example.rs': 'b'.repeat(64) } },
		},
		output: '/fresh',
		source: { head: 'a'.repeat(40), files: { 'sample.rs': 'b'.repeat(64) } },
		driver: { path: '/driver', sha256: 'c'.repeat(64) },
		example: { path: '/example', sha256: 'd'.repeat(64) },
		node: { path: '/node', sha256: 'e'.repeat(64) },
		chrome: '/chrome',
	});
}
test('default coverage retains every ID/grid/protocol and independent exit, lifecycle and glyph probes', () => {
	const matrix = scenarioMatrix(plan());
	expect(matrix).toHaveLength(400);
	expect(new Set(matrix.map((entry) => entry.scenarioId))).toEqual(new Set(scenarioIds));
	expect(
		new Set(
			matrix.map(
				(entry) =>
					entry.scenarioId +
					'|' +
					entry.keys +
					'|' +
					entry.columns +
					'x' +
					entry.rows +
					'|' +
					variantLabel(entry.variant),
			),
		).size,
	).toBe(400);
	expect(matrix.filter((entry) => entry.scenarioId === 'SC07')).toHaveLength(32);
	expect(matrix.filter((entry) => entry.scenarioId === 'SC25')).toHaveLength(96);
	expect(matrix.filter((entry) => entry.scenarioId === 'SC26')).toHaveLength(40);
	expect(matrix.filter((entry) => entry.scenarioId === 'SC22')).toHaveLength(48);
});
test('a declared variant narrows to that real case and unknown labels reject', () => {
	const base = plan();
	expect(
		scenarioMatrix({
			...base,
			cases: ['SC25'],
			grids: [{ columns: 76, rows: 30 }],
			variantLabels: ['wide-dropdown-folder'],
		}),
	).toHaveLength(2);
	expect(() =>
		scenarioMatrix({ ...base, cases: ['SC25'], variantLabels: ['lifecycle-close'] }),
	).toThrow('declared cases');
});
