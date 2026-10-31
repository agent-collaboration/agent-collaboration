/** Mechanically ported input/checkpoint order; terminal waits replace scheduling delays. */
import type { JourneyDefinition } from './journey-definition.ts';
export default {
	name: 'hard-120x36',
	groups: [
		{
			checkpoint: 'component-quota-hard-browse',
			actions: [],
		},
		{
			checkpoint: 'component-quota-hard-beta-windows',
			actions: [
				{
					kind: 'key',
					key: 'Down',
					repeat: 1,
				},
			],
		},
		{
			checkpoint: 'component-quota-hard-beta-extra',
			actions: [],
		},
		{
			checkpoint: 'component-quota-hard-pool-details',
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
			checkpoint: 'component-quota-hard-pool-details-complete',
			actions: [],
		},
		{
			checkpoint: 'component-quota-hard-pool-details-middle',
			actions: [],
		},
		{
			checkpoint: 'component-quota-hard-pool-details-bottom',
			actions: [
				{
					kind: 'key',
					key: 'Down',
					repeat: 256,
				},
			],
		},
		{
			checkpoint: 'component-quota-hard-pool-closed',
			actions: [
				{
					kind: 'key',
					key: 'Escape',
					repeat: 1,
				},
			],
		},
		{
			checkpoint: 'component-quota-hard-details',
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
	],
	finish: [
		{
			kind: 'key',
			key: 'Escape',
			repeat: 1,
		},
		{
			kind: 'wait',
			pattern: 'Quota status',
		},
		{
			kind: 'text',
			text: 'q',
		},
	],
} satisfies JourneyDefinition;
