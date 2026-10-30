import { expect, test } from 'vitest';

import { checkPreparationReadback, preparationProofSchema } from './preparation-readback.ts';
const expected = {
	client: 'codex-native',
	machine: 'local',
	directory: '/repo/project-a',
	runMode: 'foreground',
} satisfies import('./preparation-readback.ts').PreparationRequest;
test('Start proof requires exactly one recorded invocation and every committed field', () => {
	expect(() =>
		checkPreparationReadback({ callCount: 1, request: expected }, expected),
	).not.toThrow();
	for (const callCount of [0, 2])
		expect(() => checkPreparationReadback({ callCount, request: expected }, expected)).toThrow();
	for (const field of ['client', 'machine', 'directory'])
		expect(() =>
			checkPreparationReadback(
				{ callCount: 1, request: { ...expected, [field]: 'foreign' } },
				expected,
			),
		).toThrow('committed fields');
	expect(() =>
		checkPreparationReadback(
			{ callCount: 1, request: expected, dispatchSucceeded: true },
			expected,
		),
	).toThrow();
});
test('pending is an explicit coverage gap, never a dispatch receipt', () => {
	expect(preparationProofSchema.parse({ kind: 'pending-s6' })).toEqual({ kind: 'pending-s6' });
	expect(() =>
		preparationProofSchema.parse({ kind: 'recorded', relativePath: '../other.json', expected }),
	).toThrow();
	expect(() => checkPreparationReadback({ kind: 'pending-s6' }, expected)).toThrow();
});
