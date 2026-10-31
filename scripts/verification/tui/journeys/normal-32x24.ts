/** Mechanically ported input/checkpoint order; terminal waits replace scheduling delays. */
import type { JourneyDefinition } from './journey-definition.ts';
export default {
	name: 'normal-32x24',
	groups: [
		{
			checkpoint: 'component-quota-browse',
			actions: [],
		},
		{
			checkpoint: 'component-quota-browse-credit-window',
			actions: [
				{
					kind: 'key',
					key: 'PageDown',
					repeat: 1,
				},
			],
		},
		{
			checkpoint: 'component-quota-browse-pace-heading',
			actions: [
				{
					kind: 'key',
					key: 'PageUp',
					repeat: 256,
				},
			],
		},
		{
			checkpoint: 'component-quota-browse-pace-value',
			actions: [],
		},
		{
			checkpoint: 'component-quota-browse-page-returned',
			actions: [],
		},
		{
			checkpoint: 'component-quota-next-account',
			actions: [
				{
					kind: 'key',
					key: 'Down',
					repeat: 1,
				},
			],
		},
		{
			checkpoint: 'component-quota-restored-account',
			actions: [
				{
					kind: 'key',
					key: 'Up',
					repeat: 1,
				},
			],
		},
		{
			checkpoint: 'component-quota-details-top',
			actions: [
				{
					kind: 'key',
					key: 'F4',
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
			checkpoint: 'component-quota-details-bottom',
			actions: [
				{
					kind: 'key',
					key: 'Down',
					repeat: 256,
				},
			],
		},
		{
			checkpoint: 'component-quota-help-top',
			actions: [
				{
					kind: 'key',
					key: 'Escape',
					repeat: 1,
				},
				{
					kind: 'key',
					key: 'F1',
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
			checkpoint: 'component-quota-help-bottom',
			actions: [
				{
					kind: 'key',
					key: 'Down',
					repeat: 256,
				},
			],
		},
		{
			checkpoint: 'component-quota-floor-original',
			actions: [
				{
					kind: 'key',
					key: 'Escape',
					repeat: 1,
				},
				{
					kind: 'key',
					key: 'Ctrl+E',
					repeat: 1,
				},
				{
					kind: 'key',
					key: 'Down',
					repeat: 256,
				},
			],
		},
		{
			checkpoint: 'component-quota-floor-draft',
			actions: [
				{
					kind: 'key',
					key: 'Right',
					repeat: 1,
				},
			],
		},
		{
			checkpoint: 'component-quota-floor-cancelled',
			actions: [
				{
					kind: 'key',
					key: 'Escape',
					repeat: 1,
				},
				{
					kind: 'key',
					key: 'Ctrl+E',
					repeat: 1,
				},
				{
					kind: 'key',
					key: 'Down',
					repeat: 256,
				},
			],
		},
		{
			checkpoint: 'component-quota-floor-saved',
			actions: [
				{
					kind: 'key',
					key: 'Right',
					repeat: 1,
				},
				{
					kind: 'key',
					key: 'Enter',
					repeat: 1,
				},
				{
					kind: 'wait',
					pattern: 'Quota status',
				},
				{
					kind: 'key',
					key: 'Ctrl+E',
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
			checkpoint: 'component-quota-options-identity',
			actions: [
				{
					kind: 'key',
					key: 'Escape',
					repeat: 1,
				},
				{
					kind: 'key',
					key: 'Ctrl+R',
					repeat: 1,
				},
			],
		},
		{
			checkpoint: 'component-quota-reset-inventory',
			actions: [
				{
					kind: 'key',
					key: 'Down',
					repeat: 256,
				},
			],
		},
		{
			checkpoint: 'component-quota-reset-last-page',
			actions: [
				{
					kind: 'key',
					key: 'PageDown',
					repeat: 1,
				},
				{
					kind: 'key',
					key: 'PageDown',
					repeat: 1,
				},
				{
					kind: 'key',
					key: 'PageDown',
					repeat: 1,
				},
				{
					kind: 'key',
					key: 'PageDown',
					repeat: 1,
				},
				{
					kind: 'key',
					key: 'PageDown',
					repeat: 1,
				},
				{
					kind: 'key',
					key: 'PageDown',
					repeat: 1,
				},
				{
					kind: 'key',
					key: 'Up',
					repeat: 256,
				},
				{
					kind: 'key',
					key: 'Down',
					repeat: 1,
				},
				{
					kind: 'key',
					key: 'Down',
					repeat: 1,
				},
				{
					kind: 'key',
					key: 'Down',
					repeat: 1,
				},
				{
					kind: 'wait',
					pattern: 'Options · Resets.*alpha-tag.*\\[voucher-7\\]',
				},
				{
					kind: 'key',
					key: 'Down',
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
			checkpoint: 'component-quota-credits-identity',
			actions: [
				{
					kind: 'key',
					key: 'Tab',
					repeat: 1,
				},
			],
		},
		{
			checkpoint: 'component-quota-credits-balance',
			actions: [
				{
					kind: 'key',
					key: 'Up',
					repeat: 256,
				},
			],
		},
		{
			checkpoint: 'component-quota-policy-original',
			actions: [
				{
					kind: 'key',
					key: 'Down',
					repeat: 256,
				},
				{
					kind: 'key',
					key: 'Enter',
					repeat: 1,
				},
				{
					kind: 'key',
					key: 'Escape',
					repeat: 1,
				},
			],
		},
		{
			checkpoint: 'component-quota-policy-choice',
			actions: [
				{
					kind: 'key',
					key: 'Right',
					repeat: 1,
				},
			],
		},
		{
			checkpoint: 'component-quota-policy-no-match',
			actions: [
				{
					kind: 'text',
					text: 'zznomatchzz',
				},
			],
		},
		{
			checkpoint: 'component-quota-policy-choice-returned',
			actions: [
				{
					kind: 'key',
					key: 'Enter',
					repeat: 1,
				},
				{
					kind: 'key',
					key: 'Escape',
					repeat: 1,
				},
			],
		},
		{
			checkpoint: 'component-quota-policy-draft',
			actions: [
				{
					kind: 'key',
					key: 'Right',
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
			checkpoint: 'component-quota-policy-cancelled',
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
					key: 'Escape',
					repeat: 1,
				},
			],
		},
		{
			checkpoint: 'component-quota-policy-saved',
			actions: [
				{
					kind: 'key',
					key: 'Right',
					repeat: 1,
				},
				{
					kind: 'key',
					key: 'Enter',
					repeat: 1,
				},
				{
					kind: 'key',
					key: 'Enter',
					repeat: 1,
				},
				{
					kind: 'wait',
					pattern: 'Options · Credits',
				},
				{
					kind: 'key',
					key: 'Enter',
					repeat: 1,
				},
				{
					kind: 'key',
					key: 'Escape',
					repeat: 1,
				},
			],
		},
		{
			checkpoint: 'component-quota-refresh-result',
			actions: [
				{
					kind: 'key',
					key: 'Escape',
					repeat: 1,
				},
				{
					kind: 'text',
					text: 'r',
				},
			],
		},
		{
			checkpoint: 'component-quota-credits-refreshed-balance',
			actions: [
				{
					kind: 'key',
					key: 'Up',
					repeat: 256,
				},
			],
		},
		{
			checkpoint: 'component-quota-credits-refreshed-freshness',
			actions: [],
		},
		{
			checkpoint: 'component-quota-credits-refreshed-observation',
			actions: [
				{
					kind: 'key',
					key: 'Down',
					repeat: 1,
				},
			],
		},
		{
			checkpoint: 'component-quota-credits-policy-boundary',
			actions: [
				{
					kind: 'key',
					key: 'Down',
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
			checkpoint: 'component-quota-reset-review',
			actions: [
				{
					kind: 'key',
					key: 'Tab',
					repeat: 1,
				},
				{
					kind: 'wait',
					pattern: 'Options · Resets.*Review',
				},
				{
					kind: 'key',
					key: 'Down',
					repeat: 256,
				},
			],
		},
		{
			checkpoint: 'component-quota-confirm-no',
			actions: [
				{
					kind: 'key',
					key: 'Enter',
					repeat: 1,
				},
				{
					kind: 'key',
					key: 'Down',
					repeat: 256,
				},
			],
		},
		{
			checkpoint: 'component-quota-confirm-no-returned',
			actions: [
				{
					kind: 'key',
					key: 'Enter',
					repeat: 1,
				},
				{
					kind: 'key',
					key: 'Down',
					repeat: 256,
				},
			],
		},
		{
			checkpoint: 'component-quota-confirm-yes',
			actions: [
				{
					kind: 'key',
					key: 'Enter',
					repeat: 1,
				},
				{
					kind: 'wait',
					pattern: 'Options · Resets.*Review',
				},
				{
					kind: 'key',
					key: 'Enter',
					repeat: 1,
				},
				{
					kind: 'key',
					key: 'Down',
					repeat: 256,
				},
				{
					kind: 'wait',
					pattern: 'Confirm reset.*Reset quota\\?.*No.*Yes.*Cancel.*Confirm',
				},
				{
					kind: 'key',
					key: 'Right',
					repeat: 1,
				},
				{
					kind: 'key',
					key: 'Down',
					repeat: 256,
				},
			],
		},
		{
			checkpoint: 'component-quota-confirm-help-top',
			actions: [
				{
					kind: 'key',
					key: 'F1',
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
			checkpoint: 'component-quota-confirm-help-identity',
			actions: [
				{
					kind: 'key',
					key: 'Down',
					repeat: 1,
				},
			],
		},
		{
			checkpoint: 'component-quota-confirm-help-returned',
			actions: [
				{
					kind: 'key',
					key: 'Escape',
					repeat: 1,
				},
				{
					kind: 'key',
					key: 'Down',
					repeat: 256,
				},
			],
		},
		{
			checkpoint: 'component-quota-reset-result',
			actions: [
				{
					kind: 'key',
					key: 'Enter',
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
			checkpoint: 'component-quota-reset-result-consumption',
			actions: [],
		},
		{
			checkpoint: 'component-quota-reset-result-bottom',
			actions: [
				{
					kind: 'key',
					key: 'Down',
					repeat: 256,
				},
			],
		},
		{
			checkpoint: 'component-quota-returned',
			actions: [
				{
					kind: 'key',
					key: 'Escape',
					repeat: 1,
				},
				{
					kind: 'wait',
					pattern: 'Options · Resets.*Close.*Inspect',
				},
				{
					kind: 'key',
					key: 'Escape',
					repeat: 1,
				},
			],
		},
	],
	finish: [
		{
			kind: 'text',
			text: 'q',
		},
	],
} satisfies JourneyDefinition;
