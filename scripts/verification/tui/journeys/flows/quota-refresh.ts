import { journeyGridSizes } from '../journey-definition.ts';
import { absentAtBase, flowReference } from './flow-definition.ts';
import type { FlowJourney } from './flow-definition.ts';
const quotaRefreshDrafts: readonly FlowJourney[] = (
	['all-success', 'partial', 'all-failed', 'setup-failed'] as const
).map((outcome) => ({
	journeyId: 'quota-refresh-' + outcome,
	requirements: ['U17/R18'],
	surface: 'quota',
	availability: absentAtBase('S7'),
	reference: flowReference(
		'R6,R18; Program Design U17 typed account outcomes',
		'U17: duplicate while held, real CLI adapter partial/all-failed/setup result mapping',
	),
	grids: [...journeyGridSizes],
	keys: ['legacy', 'kitty'],
	fixtures: [
		'real CLI refresh adapter with isolated fixture provider',
		'causal first-request hold/release',
		'per-account outcomes from this invocation, not persisted older rows',
		'fixed selected account and changed report/Details values',
	],
	steps: [
		{
			kind: 'fixture',
			operation: 'refresh-' + outcome,
			purpose:
				'All-success, one healthy/one failing, all failing, or overall setup/read failure; exactly one service call.',
		},
		{
			kind: 'fixture',
			operation: 'hold-first-refresh',
			purpose: 'Causal pending acknowledgment, never a sleep.',
		},
		{ kind: 'press', key: 'R' },
		{ kind: 'capture', checkpoint: 'refresh-pending', checks: ['refresh-visible-and-disabled'] },
		{ kind: 'press', key: 'R' },
		{ kind: 'press', key: 'F10' },
		{
			kind: 'capture',
			checkpoint: 'refresh-menu-unavailable',
			checks: ['refresh-visible-and-disabled'],
		},
		{ kind: 'click', target: 'Refresh' },
		{ kind: 'readback', port: 'quota-refresh', checks: ['refresh-one-in-flight'] },
		{
			kind: 'fixture',
			operation: 'release-first-refresh',
			purpose: 'Release only after both duplicate activations have been acknowledged.',
		},
		{
			kind: 'readback',
			port: 'quota-refresh',
			checks: ['typed-refresh-result', 'refresh-one-in-flight', 'no-retry'],
		},
		{
			kind: 'capture',
			checkpoint: 'refresh-complete',
			checks: ['refresh-values-and-details', 'typed-refresh-result'],
		},
		{
			kind: 'fixture',
			operation: 'repeat-through-menu-with-details-open',
			purpose:
				'A fresh owned invocation proves the menu route and open Details refresh; independent call count starts at zero.',
		},
		{
			kind: 'readback',
			port: 'quota-refresh',
			checks: ['typed-refresh-result', 'refresh-values-and-details', 'no-retry'],
		},
	],
	wrongOutcomes: [
		'duplicate refresh while held',
		'screen leaves/blanks',
		'selection changes',
		'stale browse or open Details',
		'partial service Ok converted to Refreshed',
		'all-failed loses account list',
		'setup failure invents attempted accounts',
		'human stdout parsed as typed outcomes',
		'automatic retry',
	],
	admissionInputs: [
		'S7 fixture provider plus actual CLI adapter and causal release/readback; a mock refresher alone cannot prove adapter result mapping.',
	],
}));

export const quotaRefreshJourneys: readonly FlowJourney[] = quotaRefreshDrafts.map((journey) =>
	Object.assign({}, journey, {
		steps: journey.steps.flatMap((step, index): FlowJourney['steps'] => {
			if (
				step.kind === 'press' &&
				step.key === 'R' &&
				journey.steps
					.slice(0, index)
					.every((previous) => previous.kind !== 'press' || previous.key !== 'R')
			)
				return [
					step,
					{
						kind: 'fixture',
						operation: 'await-refresh-held',
						purpose:
							'Await the actual first request/setup hold through its flushed fence, then retain a separate native F12.',
					},
				];
			if (step.kind === 'capture' && step.checkpoint === 'refresh-menu-unavailable')
				return [
					{
						kind: 'fixture',
						operation: 'inspect-pending-refresh-menu',
						purpose:
							'Traverse the actual pending menu and reveal unavailable Refresh before clicking its painted cells.',
					},
					step,
				];
			return [step];
		}),
	}),
);
