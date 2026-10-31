import { journeyGridSizes } from '../journey-definition.ts';
import { absentAtBase, flowReference } from './flow-definition.ts';
import type { FlowJourney } from './flow-definition.ts';
const shared = {
	requirements: ['U16/R17'],
	surface: 'quota',
	availability: absentAtBase('S7'),
	reference: flowReference(
		'R17; Program Design Account actions from quota Details',
		'U16: Actions, captured target, Remove, reselection, RequiresDisabled',
	),
	grids: [...journeyGridSizes],
	keys: ['legacy', 'kitty'],
	fixtures: [
		'captured A and separately selected B',
		'recording command/clipboard ports',
		'typed account-command result and refreshed report',
	],
	wrongOutcomes: [
		'enabled account exposes Remove',
		'wrong account receives a command',
		'Cancel is not primary/initially focused',
		'Ctrl+Enter removes',
		'wrong reselection after removal',
		'retry after refusal',
	],
	admissionInputs: [
		'S7 account-command/clipboard fixtures; declared keys for actions without a fixed key in R17 must be exercised and equal across projections.',
	],
} satisfies Omit<FlowJourney, 'journeyId' | 'steps'>;
export const accountActionJourneys: readonly FlowJourney[] = [
	{
		...shared,
		journeyId: 'quota-actions-every-command',
		steps: [
			{
				kind: 'fixture',
				operation: 'open-details-for-enabled-a-then-select-b-and-reload',
				purpose: 'Keep the captured account target while browse changes.',
			},
			{ kind: 'click', target: 'Actions ▾' },
			{
				kind: 'capture',
				checkpoint: 'enabled-actions-menu',
				checks: ['account-actions-inventory', 'captured-account-target'],
			},
			{ kind: 'click', target: 'Options…' },
			{
				kind: 'capture',
				checkpoint: 'captured-options',
				checks: ['options-and-floor-captured-target'],
			},
			{ kind: 'press', key: 'Escape' },
			{
				kind: 'fixture',
				operation: 'open-details-for-enabled-a-then-select-b-and-reload',
				purpose:
					'Options replaced Details; return to Browse, open A through real Enter, then move Browse to B and reload while A is captured.',
			},
			{ kind: 'click', target: 'Actions ▾' },
			{ kind: 'click', target: 'Weekly floor…' },
			{
				kind: 'capture',
				checkpoint: 'captured-floor',
				checks: ['options-and-floor-captured-target'],
			},
			{ kind: 'press', key: 'Escape' },
			{
				kind: 'fixture',
				operation: 'open-details-for-enabled-a-then-select-b-and-reload',
				purpose:
					'Floor replaced Details; reopen captured A through real Enter before the remaining actions, with Browse again on B.',
			},
			{ kind: 'click', target: 'Actions ▾' },
			{ kind: 'click', target: 'Copy account ID' },
			{ kind: 'readback', port: 'clipboard', checks: ['clipboard-full-account-id'] },
			{ kind: 'click', target: 'Actions ▾' },
			{ kind: 'click', target: 'Disable account' },
			{
				kind: 'readback',
				port: 'account-command',
				checks: ['status-toggle', 'captured-account-target', 'no-retry'],
			},
			{ kind: 'click', target: 'Actions ▾' },
			{
				kind: 'capture',
				checkpoint: 'disabled-actions-menu',
				checks: ['account-actions-inventory'],
			},
			{ kind: 'click', target: 'Enable account' },
			{
				kind: 'readback',
				port: 'account-command',
				checks: ['status-toggle', 'captured-account-target', 'no-retry'],
			},
			{
				kind: 'fixture',
				operation: 'remove-captured-a-from-reloaded-report',
				purpose:
					'Every captured-account action becomes unavailable with reason account no longer listed.',
			},
			{ kind: 'capture', checkpoint: 'captured-account-gone', checks: ['captured-account-target'] },
		],
	},
	{
		...shared,
		journeyId: 'quota-remove-cancel-primary',
		steps: [
			{
				kind: 'fixture',
				operation: 'open-details-for-disabled-a',
				purpose: 'Removal is reachable only for the disabled captured target.',
			},
			{ kind: 'click', target: 'Actions ▾' },
			{ kind: 'click', target: 'Remove account…' },
			{
				kind: 'capture',
				checkpoint: 'remove-confirmation-initial',
				checks: ['cancel-primary-and-no-remove'],
			},
			{ kind: 'press', key: 'Ctrl+Enter' },
			{ kind: 'readback', port: 'account-command', checks: ['cancel-primary-and-no-remove'] },
			{
				kind: 'capture',
				checkpoint: 'remove-cancel-restores-details',
				checks: ['captured-account-target'],
			},
		],
	},
	...(['middle', 'last', 'only', 'requires-disabled-refusal'] as const).map((position) =>
		Object.assign({}, shared, {
			journeyId: 'quota-remove-' + position,
			steps: [
				{
					kind: 'fixture',
					operation: 'disabled-remove-' + position,
					purpose:
						'Expected replacement is the account at the old position, last survivor, or none only when empty; refusal retains the target.',
				},
				{ kind: 'press', key: 'Enter' },
				{ kind: 'click', target: 'Actions ▾' },
				{ kind: 'click', target: 'Remove account…' },
				{
					kind: 'capture',
					checkpoint: 'remove-confirmation',
					checks: ['cancel-primary-and-no-remove'],
				},
				{ kind: 'focus', target: 'Remove', via: 'Tab' },
				{ kind: 'press', key: 'Enter' },
				{
					kind: 'readback',
					port: 'account-command',
					checks: [
						'captured-account-target',
						'no-retry',
						position === 'requires-disabled-refusal'
							? 'disabled-first-refusal'
							: 'explicit-remove-and-reselection',
					],
				},
				{
					kind: 'capture',
					checkpoint: 'remove-return',
					checks: [
						position === 'requires-disabled-refusal'
							? 'disabled-first-refusal'
							: 'explicit-remove-and-reselection',
					],
				},
			] satisfies FlowJourney['steps'],
		}),
	),
];
