import { existsSync } from 'node:fs';
import { join } from 'node:path';

import { expect, test } from 'vitest';

import { sourceDigest } from '../../evidence/source-binding.ts';
import { protocolPlan, temporaryRoot } from '../../fixtures/receipt-fixtures.ts';
import { authoredFlowPlanSchema } from './authored-flow-plan.ts';
import type { AuthoredFlowPlan } from './authored-flow-plan.ts';
import { authoredFlowMatrix, runAuthoredFlow } from './authored-flow-run.ts';
import { authoredFlows } from './flow-catalogue.ts';
import { pickerFlowBindings, pickerFilterTextExpectations } from './picker-flow-bindings.ts';

function controlledPlan(journey: string): AuthoredFlowPlan {
	const root = temporaryRoot(),
		fixture = protocolPlan(root);
	return authoredFlowPlanSchema.parse({
		productRepository: root,
		productSource: fixture.source,
		productSourceDigest: sourceDigest(fixture.source),
		observerRepository: root,
		observerSource: fixture.source,
		observerSourceDigest: sourceDigest(fixture.source),
		driver: fixture.capture_driver,
		product: fixture.harness_binary,
		node: fixture.node,
		chrome: '/unneeded/no-native-run',
		output: join(root, 'not-created'),
		journey,
		grids: ['120x36', '76x30', '48x12', '32x24'],
		keys: ['legacy', 'kitty'],
	});
}
test('all authored flow data selects through one matrix without copied steps', () => {
	for (const journey of authoredFlows) {
		const matrix = authoredFlowMatrix(controlledPlan(journey.journeyId));
		expect(matrix).toHaveLength(8);
		expect(matrix.every((invocation) => invocation.journey === journey)).toBe(true);
	}
});
test('unsupported native fixture rejects before even creating an output directory', async () => {
	const plan = controlledPlan('quota-refresh-partial');
	await expect(runAuthoredFlow(plan)).rejects.toThrow('fixture admission gap: no native adapter');
	expect(existsSync(plan.output)).toBe(false);
});
test('the stopped native adapter declares its independent first-overlay and exact captured source', () => {
	const plan = controlledPlan('stopped-claude-details-open-only');
	const invocation = authoredFlowMatrix(plan)[0];
	if (invocation === undefined) throw new Error('controlled matrix empty');
	const bindings = pickerFlowBindings(plan, invocation, plan.output, join(plan.output, 'receipts'));
	expect(bindings?.target.storyboardVariant).toBe('stopped-native-claude');
	expect(bindings?.inputs.get(4)).toEqual({
		kind: 'exit',
		expected: [
			{
				event: {
					kind: 'key',
					code: { kind: 'fixed', name: process.platform === 'darwin' ? 'Return' : 'Enter' },
					modifiers: [],
					phase: 'press',
				},
				surface: { kind: 'overlay', overlayId: '1' },
				target: 'action:details-open',
				action: 'details-open',
			},
		],
	});
	expect(bindings?.checks.has('captured-session-outcome')).toBe(true);
});
test('raw uppercase infers Shift while explicit CSI-u text keeps its encoded modifiers', () => {
	expect(pickerFilterTextExpectations('Sc ', 0).map((expected) => expected.event)).toEqual([
		{ kind: 'key', code: { kind: 'text' }, modifiers: ['shift'], phase: 'press' },
		{ kind: 'key', code: { kind: 'text' }, modifiers: [], phase: 'press' },
		{ kind: 'key', code: { kind: 'text' }, modifiers: [], phase: 'press' },
	]);
	expect(pickerFilterTextExpectations('S', 1)[0]?.event).toEqual(
		pickerFilterTextExpectations('S', 0)[0]?.event,
	);
	expect(pickerFilterTextExpectations('S', 8)[0]?.event).toEqual({
		kind: 'key',
		code: { kind: 'text' },
		modifiers: [],
		phase: 'press',
	});
});
