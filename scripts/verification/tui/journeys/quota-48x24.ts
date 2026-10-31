/** Ported input/checkpoint order; active-field readiness replaces a text-gap matcher. */
import type { JourneyDefinition } from './journey-definition.ts';
export default {
	name: 'quota-48x24',
	groups: [
		{
			checkpoint: 'compact-quota-all',
			actions: [
				{
					kind: 'wait',
					pattern: 'All.*Disabled: Shown.*4\\/4( accounts| ▾)',
				},
				{
					kind: 'key',
					key: 'Up',
					repeat: 1,
				},
				{
					kind: 'key',
					key: 'Up',
					repeat: 1,
				},
				{
					kind: 'key',
					key: 'Up',
					repeat: 1,
				},
			],
		},
		{
			checkpoint: 'compact-quota-pool-details',
			actions: [
				{
					kind: 'key',
					key: 'F5',
					repeat: 1,
				},
				{
					kind: 'key',
					key: 'Up',
					repeat: 256,
				},
			],
		},
		{
			checkpoint: 'compact-quota-pool-details-middle',
			actions: [],
		},
		{
			checkpoint: 'compact-quota-pool-details-bottom',
			actions: [
				{
					kind: 'key',
					key: 'Down',
					repeat: 256,
				},
			],
		},
		{
			checkpoint: 'compact-quota-pool-closed',
			actions: [
				{
					kind: 'key',
					key: 'Escape',
					repeat: 1,
				},
			],
		},
		{
			checkpoint: 'compact-quota-filters-block-pool',
			actions: [
				{
					kind: 'key',
					key: 'Ctrl+F',
					repeat: 1,
				},
				{
					kind: 'wait',
					pattern: 'Account filters.*Account provider',
				},
				{
					kind: 'key',
					key: 'F5',
					repeat: 1,
				},
			],
		},
		{
			checkpoint: 'compact-quota-provider-no-match',
			actions: [
				{
					kind: 'key',
					key: 'Escape',
					repeat: 1,
				},
				{
					kind: 'wait',
					pattern: 'All.*Disabled: Shown.*4\\/4( accounts| ▾).*Claude Delta',
				},
				{
					kind: 'key',
					key: 'Ctrl+F',
					repeat: 1,
				},
				{
					kind: 'key',
					key: 'Enter',
					repeat: 1,
				},
				{
					kind: 'text',
					text: 'zznomatchzz',
				},
			],
		},
		{
			checkpoint: 'compact-quota-chooser',
			actions: [
				{
					kind: 'key',
					key: 'Escape',
					repeat: 1,
				},
				{
					kind: 'key',
					key: 'Enter',
					repeat: 1,
				},
				{
					kind: 'key',
					key: 'Down',
					repeat: 1,
				},
				{
					kind: 'key',
					key: 'Enter',
					repeat: 1,
				},
				{
					kind: 'key',
					key: 'Down',
					repeat: 1,
				},
			],
		},
		{
			checkpoint: 'compact-quota-help',
			actions: [
				{
					kind: 'key',
					key: 'Up',
					repeat: 1,
				},
				{
					kind: 'key',
					key: 'F1',
					repeat: 1,
				},
				{
					kind: 'wait',
					pattern: 'Quota Help',
				},
				{
					kind: 'key',
					key: 'Down',
					repeat: 3,
				},
			],
		},
		{
			checkpoint: 'compact-quota-disabled-choice',
			actions: [
				{
					kind: 'key',
					key: 'Escape',
					repeat: 1,
				},
				{
					kind: 'field',
					dialog: 'Account filters',
					label: 'Account provider',
					value: 'OpenAI',
				},
				{
					kind: 'key',
					key: 'Enter',
					repeat: 1,
				},
				{
					kind: 'key',
					key: 'Down',
					repeat: 1,
				},
				{
					kind: 'key',
					key: 'Enter',
					repeat: 1,
				},
				{
					kind: 'key',
					key: 'Tab',
					repeat: 1,
				},
				{
					kind: 'key',
					key: 'Enter',
					repeat: 1,
				},
			],
		},
		{
			checkpoint: 'compact-quota-hidden-choice',
			actions: [
				{
					kind: 'key',
					key: 'Down',
					repeat: 1,
				},
			],
		},
		{
			checkpoint: 'compact-quota-claude',
			actions: [
				{
					kind: 'key',
					key: 'Enter',
					repeat: 1,
				},
				{
					kind: 'key',
					key: 'Tab',
					repeat: 1,
				},
				{
					kind: 'key',
					key: 'Enter',
					repeat: 1,
				},
			],
		},
		{
			checkpoint: 'compact-quota-cancelled',
			actions: [
				{
					kind: 'key',
					key: 'Ctrl+F',
					repeat: 1,
				},
				{
					kind: 'key',
					key: 'Enter',
					repeat: 1,
				},
				{
					kind: 'key',
					key: 'Up',
					repeat: 1,
				},
				{
					kind: 'key',
					key: 'Enter',
					repeat: 1,
				},
				{
					kind: 'field',
					dialog: 'Account filters',
					label: 'Account provider',
					value: 'OpenAI',
				},
				{
					kind: 'key',
					key: 'Escape',
					repeat: 1,
				},
			],
		},
		{
			checkpoint: 'compact-quota-details',
			actions: [
				{
					kind: 'key',
					key: 'F4',
					repeat: 1,
				},
			],
		},
	],
	finish: [
		{
			kind: 'key',
			key: 'Escape',
			repeat: 1,
		},
		{
			kind: 'wait',
			pattern: 'Claude.*Disabled: Hidden.*1\\/4( accounts| ▾).*Claude Gamma',
		},
		{
			kind: 'text',
			text: 'q',
		},
	],
} satisfies JourneyDefinition;
