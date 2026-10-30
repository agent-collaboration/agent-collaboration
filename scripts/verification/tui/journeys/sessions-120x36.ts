/** Mechanically ported input/checkpoint order; terminal waits replace scheduling delays. */
import type { JourneyDefinition } from '../journey-definition.ts';
export default {
	name: 'sessions-120x36',
	groups: [
		{
			checkpoint: 'compact-sessions-all',
			actions: [],
		},
		{
			checkpoint: 'compact-sessions-codex-details',
			actions: [
				{
					kind: 'key',
					key: 'F4',
					repeat: 1,
				},
			],
		},
		{
			checkpoint: 'compact-sessions-fork',
			actions: [
				{
					kind: 'key',
					key: 'Shift+Tab',
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
			checkpoint: 'compact-sessions-fork-choice',
			actions: [
				{
					kind: 'key',
					key: 'Tab',
					repeat: 1,
				},
			],
		},
		{
			checkpoint: 'compact-sessions-new',
			actions: [
				{
					kind: 'key',
					key: 'Escape',
					repeat: 1,
				},
				{
					kind: 'key',
					key: 'Escape',
					repeat: 1,
				},
				{
					kind: 'wait',
					pattern: '6\\/6.*Codex Alpha',
				},
				{
					kind: 'key',
					key: 'Ctrl+N',
					repeat: 1,
				},
			],
		},
		{
			checkpoint: 'compact-sessions-new-machine',
			actions: [
				{
					kind: 'key',
					key: 'Shift+Tab',
					repeat: 1,
				},
				{
					kind: 'key',
					key: 'Shift+Tab',
					repeat: 1,
				},
				{
					kind: 'key',
					key: 'Shift+Tab',
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
			checkpoint: 'compact-sessions-new-no-match',
			actions: [
				{
					kind: 'text',
					text: 'zznomatchzz',
				},
			],
		},
		{
			checkpoint: 'compact-sessions-new-returned',
			actions: [
				{
					kind: 'key',
					key: 'Escape',
					repeat: 1,
				},
			],
		},
		{
			checkpoint: 'compact-sessions-machine-sources',
			actions: [
				{
					kind: 'key',
					key: 'Escape',
					repeat: 1,
				},
				{
					kind: 'wait',
					pattern: '6\\/6.*Codex Alpha',
				},
				{
					kind: 'key',
					key: 'Ctrl+G',
					repeat: 1,
				},
			],
		},
		{
			checkpoint: 'compact-sessions-client-no-match',
			actions: [
				{
					kind: 'key',
					key: 'Escape',
					repeat: 1,
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
			checkpoint: 'compact-sessions-chooser',
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
			],
		},
		{
			checkpoint: 'compact-sessions-help',
			actions: [
				{
					kind: 'key',
					key: 'F1',
					repeat: 1,
				},
			],
		},
		{
			checkpoint: 'compact-sessions-claude',
			actions: [
				{
					kind: 'key',
					key: 'Escape',
					repeat: 1,
				},
				{
					kind: 'wait',
					pattern: 'Session filters.*Client.*Codex.*Cancel.*Apply',
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
					key: 'Tab',
					repeat: 1,
				},
				{
					kind: 'key',
					key: 'Tab',
					repeat: 1,
				},
				{
					kind: 'key',
					key: 'Tab',
					repeat: 1,
				},
				{
					kind: 'key',
					key: 'Tab',
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
			checkpoint: 'compact-sessions-cancelled',
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
					key: 'Down',
					repeat: 1,
				},
				{
					kind: 'key',
					key: 'Enter',
					repeat: 1,
				},
				{
					kind: 'wait',
					pattern: 'Session filters.*Client.*Cursor.*Cancel.*Apply',
				},
				{
					kind: 'key',
					key: 'Escape',
					repeat: 1,
				},
			],
		},
		{
			checkpoint: 'compact-sessions-details',
			actions: [
				{
					kind: 'key',
					key: 'F4',
					repeat: 1,
				},
			],
		},
		{
			checkpoint: 'compact-sessions-details-path',
			actions: [],
		},
		{
			checkpoint: 'compact-sessions-details-title',
			actions: [],
		},
		{
			checkpoint: 'compact-sessions-folder-choice',
			actions: [
				{
					kind: 'key',
					key: 'Escape',
					repeat: 1,
				},
				{
					kind: 'wait',
					pattern: '2\\/6.*Claude Gamma',
				},
				{
					kind: 'key',
					key: 'Ctrl+F',
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
				{
					kind: 'text',
					text: 'Checkout',
				},
				{
					kind: 'key',
					key: 'Enter',
					repeat: 1,
				},
			],
		},
		{
			checkpoint: 'compact-sessions-model-provider-choice',
			actions: [
				{
					kind: 'key',
					key: 'Escape',
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
			checkpoint: 'compact-sessions-source-choice',
			actions: [
				{
					kind: 'key',
					key: 'Escape',
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
			checkpoint: 'compact-sessions-runtime-choice',
			actions: [
				{
					kind: 'key',
					key: 'Escape',
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
			checkpoint: 'compact-sessions-sort-choice',
			actions: [
				{
					kind: 'key',
					key: 'Escape',
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
	],
	finish: [
		{
			kind: 'key',
			key: 'Escape',
			repeat: 1,
		},
		{
			kind: 'key',
			key: 'Escape',
			repeat: 1,
		},
		{
			kind: 'wait',
			pattern: '2\\/6.*Claude Gamma',
		},
		{
			kind: 'key',
			key: 'Ctrl+D',
			repeat: 1,
		},
	],
} satisfies JourneyDefinition;
