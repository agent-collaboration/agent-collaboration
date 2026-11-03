/** Qualified SSH Fork retains the exact source identity through Details and Fork confirmation. */
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

import {
	checkSourceQualifiedOutcome,
	parsePickerOutcomeReadback,
} from '../../sessions/source-qualified-outcome.ts';
import type { AuthoredFlowPlan } from './authored-flow-plan.ts';
import type { FlowInvocation } from './flow-executor.ts';
import type { NativeFlowBindings, NativeFlowInputBinding } from './native-flow-runtime.ts';
import {
	pickerBaseBindings,
	pickerDialogTarget,
	pickerExpectedKey,
	pickerFilterTextExpectations,
	pickerFocused,
	pickerOverlay,
	pickerScreen,
} from './picker-flow-inputs.ts';

const qualifiedForkOutcome = {
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
} as const;

export function checkQualifiedForkOutcome(value: unknown): readonly string[] {
	checkSourceQualifiedOutcome(value, qualifiedForkOutcome);
	return [];
}

export function pickerForkBindings(
	plan: AuthoredFlowPlan,
	invocation: FlowInvocation,
	evidence: string,
	receipts: string,
): NativeFlowBindings | null {
	if (invocation.journey.journeyId !== 'same-id-hosted-codex-fork') return null;
	const base = pickerBaseBindings({
		plan,
		invocation,
		evidence,
		receipts,
		variant: 'fork-ssh-profile',
	});
	const details = pickerOverlay(1),
		confirmation = pickerOverlay(2);
	const focus = (surface: typeof details, title: string): NativeFlowInputBinding => ({
		kind: 'focus',
		route: Array.from({ length: 5 }, () => ({
			key: 'Tab',
			expected: [pickerExpectedKey('Tab', surface)],
		})),
		stopWhenFocused: true,
		isFocused: (grid) => pickerFocused(grid, title, 'Fork'),
	});
	return {
		...base,
		inputs: new Map([
			[
				1,
				{
					kind: 'continuing',
					expected: [pickerExpectedKey('Enter', pickerScreen, 'session-filter', 'details')],
				},
			],
			[2, focus(details, 'Session details')],
			[
				3,
				{
					kind: 'continuing',
					expected: [pickerExpectedKey('Enter', details, 'action:details-fork', 'details-fork')],
				},
			],
			[4, focus(confirmation, 'Fork session')],
			[
				5,
				{
					kind: 'exit',
					expected: [
						pickerExpectedKey('Enter', confirmation, 'action:fork-confirm', 'fork-confirm'),
					],
				},
			],
		] as [number, NativeFlowInputBinding][]),
		targets: new Map([['Fork', (grid) => pickerDialogTarget(grid, 'Fork session', 'Fork')]]),
		fixtures: new Map([
			[
				'select-same-id-hosted-codex',
				async (context) => {
					await context.input.press('/', [
						{
							event: { kind: 'key', code: { kind: 'text' }, modifiers: [], phase: 'press' },
							surface: pickerScreen,
							target: null,
							action: 'focus-filter',
						},
					]);
					return context.input.text(
						'SSH fork source',
						pickerFilterTextExpectations('SSH fork source', context.driver.inputModes().kittyFlags),
					);
				},
			],
		]),
		readbacks: new Map([
			[
				'source-qualified-outcome',
				async (context) => {
					if (!context.driver.bytes().includes('SESSION_PICKER_OUTCOME prepared-source'))
						throw new Error('Fork did not return the actual prepared-source outcome');
					if (existsSync(join(receipts, 'source-preparation.json')))
						throw new Error('Fork acquired a New-only preparation receipt');
					return parsePickerOutcomeReadback(readFileSync(join(receipts, 'picker-outcome.json')));
				},
			],
		]),
		checks: new Map([
			['captured-session-outcome', async (_context, value) => checkQualifiedForkOutcome(value)],
		]),
	};
}
