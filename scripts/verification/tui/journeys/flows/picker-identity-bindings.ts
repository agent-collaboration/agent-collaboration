/** Same-ID HostedCodex return: the selected third row differs by both service and endpoint. */
import { readFileSync } from 'node:fs';
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

export const hostedCodexIndigoOutcome = {
	kind: 'resume',
	identity: {
		kind: 'hostedCodex',
		target: {
			endpoint: { serviceId: '00000000-0000-4000-8000-000000000002', endpointId: 'codex-source-b' },
			sessionId: '00000000-0000-4000-8000-000000000011',
		},
	},
} as const;

export function pickerIdentityBindings(
	plan: AuthoredFlowPlan,
	invocation: FlowInvocation,
	evidence: string,
	receipts: string,
): NativeFlowBindings | null {
	if (invocation.journey.journeyId !== 'same-id-hosted-codex-resume') return null;
	const base = pickerBaseBindings({
		plan,
		invocation,
		evidence,
		receipts,
		variant: 'source-qualified-outcome',
	});
	const details = pickerOverlay(1);
	return {
		...base,
		inputs: new Map<number, NativeFlowInputBinding>([
			[
				1,
				{
					kind: 'continuing',
					expected: [pickerExpectedKey('Enter', pickerScreen, 'session-filter', 'details')],
				},
			],
			[
				2,
				{
					kind: 'focus',
					route: Array.from({ length: 5 }, () => ({
						key: 'Tab',
						expected: [pickerExpectedKey('Tab', details)],
					})),
					stopWhenFocused: true,
					isFocused: (grid) => pickerFocused(grid, 'Session details', 'Resume'),
				},
			],
			[
				3,
				{
					kind: 'exit',
					expected: [pickerExpectedKey('Enter', details, 'action:details-open', 'details-open')],
				},
			],
		]),
		targets: new Map([['Resume', (grid) => pickerDialogTarget(grid, 'Session details', 'Resume')]]),
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
						'Hosted Codex Indigo',
						pickerFilterTextExpectations(
							'Hosted Codex Indigo',
							context.driver.inputModes().kittyFlags,
						),
					);
				},
			],
		]),
		readbacks: new Map([
			[
				'source-qualified-outcome',
				async () => parsePickerOutcomeReadback(readFileSync(join(receipts, 'picker-outcome.json'))),
			],
		]),
		checks: new Map([
			[
				'captured-session-outcome',
				async (_context, value) => {
					checkSourceQualifiedOutcome(value, hostedCodexIndigoOutcome);
					return [];
				},
			],
		]),
	};
}
