import { existsSync } from 'node:fs';
import { join } from 'node:path';

import { expect, test } from 'vitest';

import { sourceDigest } from '../../evidence/source-binding.ts';
import { protocolPlan, temporaryRoot } from '../../fixtures/receipt-fixtures.ts';
import { checkInputConsumptionReceipt } from '../../sessions/input-consumption-readback.ts';
import { authoredFlowPlanSchema, paintedMouseExpectationSchema } from './authored-flow-plan.ts';
import type { AuthoredFlowPlan } from './authored-flow-plan.ts';
import { authoredFlowMatrix, runAuthoredFlow } from './authored-flow-run.ts';
import { authoredFlows } from './flow-catalogue.ts';
import { pickerFlowAdmission } from './picker-flow-admission.ts';
import { pickerFlowBindings } from './picker-flow-bindings.ts';
import { pickerExpectedKey, pickerFilterTextExpectations } from './picker-flow-inputs.ts';
import { checkQualifiedForkOutcome } from './picker-fork-bindings.ts';

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
		keys: authoredFlows.find((flow) => flow.journeyId === journey)?.keys ?? ['legacy', 'kitty'],
	});
}
test('mouse preflight uses the adopted active-surface contract and rejects the retired dialog-only literal', () => {
	const expectation = {
		kind: 'painted-mouse',
		label: 'Menu',
		mouse: 'down',
		button: 'left',
		modifiers: [],
		coordinateRule: 'unique-fully-painted-label-in-active-surface',
		surface: { kind: 'screen' },
		target: 'action:menu',
		action: 'menu',
	};
	expect(paintedMouseExpectationSchema.safeParse(expectation).success).toBe(true);
	expect(
		paintedMouseExpectationSchema.safeParse({
			...expectation,
			coordinateRule: 'unique-fully-painted-label-in-active-dialog',
		}).success,
	).toBe(false);
});
test('all authored flow data selects through one matrix without copied steps', () => {
	for (const journey of authoredFlows) {
		const matrix = authoredFlowMatrix(controlledPlan(journey.journeyId));
		expect(matrix).toHaveLength(4 * journey.keys.length);
		expect(matrix.every((invocation) => invocation.journey === journey)).toBe(true);
	}
});
test('unsupported native fixture rejects before even creating an output directory', async () => {
	const plan = controlledPlan('quota-refresh-partial');
	await expect(runAuthoredFlow(plan)).rejects.toThrow('no native adapter');
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

test('all nine New routes and the hosted Resume route have complete native capabilities', () => {
	const selected = authoredFlows.filter(
		(flow) => flow.journeyId.startsWith('new-') || flow.journeyId === 'same-id-hosted-codex-resume',
	);
	expect(selected).toHaveLength(10);
	for (const journey of selected) {
		const plan = controlledPlan(journey.journeyId);
		for (const invocation of authoredFlowMatrix(plan)) {
			const binding = pickerFlowBindings(
				plan,
				invocation,
				plan.output,
				join(plan.output, 'receipts'),
			);
			expect(pickerFlowAdmission(invocation, binding)).toEqual([]);
		}
	}
});

test('qualified SSH Fork binds both actual dialog owners and rejects a same-ID wrong source outcome', () => {
	const plan = controlledPlan('same-id-hosted-codex-fork');
	const invocation = authoredFlowMatrix(plan)[0];
	if (invocation === undefined) throw new Error('empty fork matrix');
	const bindings = pickerFlowBindings(plan, invocation, plan.output, join(plan.output, 'receipts'));
	expect(pickerFlowAdmission(invocation, bindings)).toEqual([]);
	expect(bindings?.target.storyboardVariant).toBe('fork-ssh-profile');
	expect(bindings?.inputs.get(3)).toMatchObject({
		kind: 'continuing',
		expected: [{ surface: { kind: 'overlay', overlayId: '1' }, action: 'details-fork' }],
	});
	expect(bindings?.inputs.get(5)).toMatchObject({
		kind: 'exit',
		expected: [{ surface: { kind: 'overlay', overlayId: '2' }, action: 'fork-confirm' }],
	});
	const check = bindings?.checks.get('captured-session-outcome');
	if (check === undefined) throw new Error('Fork outcome checker absent');
	const expected = {
		kind: 'fork',
		identity: {
			kind: 'hostedCodex',
			target: {
				endpoint: {
					serviceId: '00000000-0000-4000-8000-000000000001',
					endpointId: 'codex-fixture-ssh',
				},
				sessionId: '00000000-0000-4000-8000-000000000011',
			},
		},
	};
	expect(checkQualifiedForkOutcome(expected)).toEqual([]);
	for (const identity of [
		{ ...expected.identity, kind: 'hostedProvider' },
		{
			...expected.identity,
			target: {
				...expected.identity.target,
				endpoint: {
					...expected.identity.target.endpoint,
					serviceId: '00000000-0000-4000-8000-000000000002',
				},
			},
		},
		{
			...expected.identity,
			target: {
				...expected.identity.target,
				endpoint: { ...expected.identity.target.endpoint, endpointId: 'codex-source-b' },
			},
		},
	])
		expect(() => checkQualifiedForkOutcome({ kind: 'fork', identity })).toThrow(
			'source-qualified identity',
		);
});

test('the R19 journey admits both protocols and nominates only its private fixture store', () => {
	const plan = controlledPlan('sessions-group-by-f6');
	for (const invocation of authoredFlowMatrix(plan)) {
		const bindings = pickerFlowBindings(
			plan,
			invocation,
			plan.output,
			join(plan.output, 'receipts'),
		);
		expect(pickerFlowAdmission(invocation, bindings)).toEqual([]);
		expect(bindings?.target.storyboardVariant).toBe('r19-grouping');
		expect(bindings?.target.sessionsPreferencesDirectory).toBe(join(plan.output, 'preferences'));
	}
});

test('every R19 regroup retains its own ACK frame before exactly one separate safe F12', () => {
	const plan = controlledPlan('sessions-group-by-f6');
	for (const invocation of authoredFlowMatrix(plan)) {
		const bindings = pickerFlowBindings(
			plan,
			invocation,
			plan.output,
			join(plan.output, 'receipts'),
		);
		for (const [index, step] of invocation.journey.steps.entries()) {
			if (step.kind !== 'press' || step.key !== 'F6') continue;
			expect(bindings?.inputs.get(index)).toMatchObject({
				kind: 'continuing',
				expected: [{ action: 'cycle-group' }],
			});
			expect(invocation.journey.steps[index + 1]).toEqual({ kind: 'press', key: 'F12' });
			expect(invocation.journey.steps[index + 2]).not.toEqual({ kind: 'press', key: 'F12' });
			expect(bindings?.inputs.get(index + 1)).toEqual({
				kind: 'continuing',
				expected: [
					{
						event: {
							kind: 'key',
							code: { kind: 'fixed', name: 'F12' },
							modifiers: [],
							phase: 'press',
						},
						surface: { kind: 'screen' },
						target: null,
						action: null,
					},
				],
			});
		}
	}
});

test.each(['picker-list-option-navigation', 'sessions-help-option-navigation'])(
	'%s binds the large-list fixture and every authored input at all four grids',
	(journey) => {
		const plan = controlledPlan(journey);
		for (const invocation of authoredFlowMatrix(plan)) {
			const binding = pickerFlowBindings(
				plan,
				invocation,
				plan.output,
				join(plan.output, 'receipts'),
			);
			expect(pickerFlowAdmission(invocation, binding)).toEqual([]);
			expect(binding?.target.storyboardVariant).toBe('large-list');
		}
	},
);

test('the toolbar binds every real menu, Help, Run dropdown and filter interaction', () => {
	const plan = controlledPlan('sessions-toolbar-and-menu');
	for (const invocation of authoredFlowMatrix(plan)) {
		const binding = pickerFlowBindings(
			plan,
			invocation,
			plan.output,
			join(plan.output, 'receipts'),
		);
		expect(pickerFlowAdmission(invocation, binding)).toEqual([]);
		expect(binding?.target.storyboardVariant).toBe('claude-new-success');
		const filterIndex = invocation.journey.steps.findIndex(
			(step) => step.kind === 'click' && step.target === 'Filter',
		);
		const filterInput = binding?.inputs.get(filterIndex);
		if (filterInput?.kind !== 'click' || filterInput.ending !== 'continue')
			throw new Error('Filter needs a continuing pointer binding');
		const location = { column: 18, row: 1 };
		expect(filterInput.expected({ ...location, width: 1 })).toEqual([
			{
				event: {
					kind: 'mouse',
					mouse: { kind: 'down', button: 'left' },
					...location,
					modifiers: [],
				},
				surface: { kind: 'screen' },
				target: 'session-filter',
				action: null,
			},
			{
				event: { kind: 'mouse', mouse: { kind: 'up', button: 'left' }, ...location, modifiers: [] },
				surface: { kind: 'screen' },
				target: null,
				action: null,
			},
		]);
	}
});

// crossterm0.29 event.rs Display: fixed keys are serialized via code.to_string().
test('picker key expectations preserve the producer Display names and reject the Rust variant spelling', () => {
	const surface = { kind: 'screen' } as const;
	const actual = {
		sequence: 174,
		event: {
			kind: 'key',
			code: { kind: 'fixed', name: 'Back Tab' },
			modifiers: ['shift'],
			phase: 'press',
		},
		surface,
		target: null,
		action: null,
	};
	const expected = pickerExpectedKey('Shift+Tab', surface);
	expect(() => checkInputConsumptionReceipt(actual, expected)).not.toThrow();
	for (const name of ['BackTab', 'Tab'])
		expect(() =>
			checkInputConsumptionReceipt(
				{ ...actual, event: { ...actual.event, code: { kind: 'fixed', name } } },
				expected,
			),
		).toThrow('event mismatch');
	for (const [key, name] of [
		['Escape', 'Esc'],
		['Enter', process.platform === 'darwin' ? 'Return' : 'Enter'],
		['Tab', 'Tab'],
		['F1', 'F1'],
		['F6', 'F6'],
		['F10', 'F10'],
		['F12', 'F12'],
		['Up', 'Up'],
		['Down', 'Down'],
		['Left', 'Left'],
		['Right', 'Right'],
	]) {
		if (key === undefined || name === undefined) throw new Error('source key case absent');
		expect(pickerExpectedKey(key, surface).event).toEqual({
			kind: 'key',
			code: { kind: 'fixed', name },
			modifiers: [],
			phase: 'press',
		});
	}
});
test('picker Option aliases and Ctrl chords keep producer code and modifier order', () => {
	const screen = { kind: 'screen' } as const;
	expect(pickerExpectedKey('Alt+Shift+Up', screen).event).toEqual({
		kind: 'key',
		code: { kind: 'fixed', name: 'Up' },
		modifiers: ['shift', 'alt'],
		phase: 'press',
	});
	expect(pickerExpectedKey('Alt+Down', screen).event).toEqual({
		kind: 'key',
		code: { kind: 'fixed', name: 'Down' },
		modifiers: ['alt'],
		phase: 'press',
	});
	expect(pickerExpectedKey('Ctrl+N', screen).event).toEqual({
		kind: 'key',
		code: { kind: 'shortcut', key: 'n' },
		modifiers: ['control'],
		phase: 'press',
	});
	expect(pickerExpectedKey('Ctrl+Shift+Enter', screen).event).toEqual({
		kind: 'key',
		code: { kind: 'fixed', name: process.platform === 'darwin' ? 'Return' : 'Enter' },
		modifiers: ['shift', 'control'],
		phase: 'press',
	});
});
