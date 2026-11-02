/** One sessions screen journey; only viewport-specific actions are declared per grid. */
import { keyPress, visibleText, typedText } from '../journey-definition.ts';
import type { ScreenJourney } from '../journey-definition.ts';

export default {
	screen: 'sessions',
	groups: [
		{ checkpoint: 'compact-sessions-all', actions: [] },
		{ checkpoint: 'compact-sessions-codex-details', actions: [keyPress('F4')] },
		{ checkpoint: 'compact-sessions-fork', actions: [keyPress('Shift+Tab'), keyPress('Enter')] },
		{ checkpoint: 'compact-sessions-fork-choice', actions: [keyPress('Tab')] },
		{
			checkpoint: 'compact-sessions-new',
			actions: [
				keyPress('Escape'),
				keyPress('Escape'),
				visibleText('6\\/6.*Codex Alpha'),
				keyPress('Ctrl+N'),
			],
		},
		{
			checkpoint: 'compact-sessions-new-machine',
			actions: [
				keyPress('Shift+Tab'),
				keyPress('Shift+Tab'),
				keyPress('Shift+Tab'),
				keyPress('Enter'),
			],
		},
		{ checkpoint: 'compact-sessions-new-no-match', actions: [typedText('zznomatchzz')] },
		{ checkpoint: 'compact-sessions-new-returned', actions: [keyPress('Escape')] },
		{
			checkpoint: 'compact-sessions-machine-sources',
			actions: [keyPress('Escape'), visibleText('6\\/6.*Codex Alpha'), keyPress('Ctrl+G')],
		},
		{
			checkpoint: 'compact-sessions-client-no-match',
			actions: [
				keyPress('Escape'),
				keyPress('Ctrl+F'),
				keyPress('Enter'),
				typedText('zznomatchzz'),
			],
		},
		{
			checkpoint: 'compact-sessions-chooser',
			actions: [keyPress('Escape'), keyPress('Enter'), keyPress('Down'), keyPress('Enter')],
		},
		{ checkpoint: 'compact-sessions-help', actions: [keyPress('F1')] },
		{
			checkpoint: 'compact-sessions-claude',
			actions: [
				keyPress('Escape'),
				visibleText('Session filters.*Client.*Codex.*Cancel.*Apply'),
				keyPress('Enter'),
				keyPress('Down'),
				keyPress('Enter'),
				keyPress('Tab'),
				keyPress('Tab'),
				keyPress('Tab'),
				keyPress('Tab'),
				keyPress('Tab'),
				keyPress('Tab'),
				keyPress('Enter'),
			],
		},
		{
			checkpoint: 'compact-sessions-cancelled',
			actions: [
				keyPress('Ctrl+F'),
				keyPress('Enter'),
				keyPress('Down'),
				keyPress('Enter'),
				visibleText('Session filters.*Client.*Cursor.*Cancel.*Apply'),
				keyPress('Escape'),
			],
		},
		{
			checkpoint: 'compact-sessions-details',
			grids: ['120x36', '76x30', '32x24'],
			actions: [keyPress('F4')],
		},
		{
			checkpoint: 'compact-sessions-details-identity',
			grids: ['48x12'],
			actions: [keyPress('F4')],
		},
		{
			checkpoint: 'compact-sessions-details-state',
			grids: ['48x12'],
			actions: [keyPress('Down'), keyPress('Down'), keyPress('Down')],
		},
		{
			checkpoint: 'compact-sessions-details-path',
			actions: [],
			actionsByGrid: {
				'76x30': [keyPress('Down'), keyPress('Down')],
				'48x12': [
					keyPress('Down'),
					keyPress('Down'),
					keyPress('Down'),
					keyPress('Down'),
					keyPress('Down'),
					keyPress('Down'),
				],
				'32x24': [
					keyPress('Down'),
					keyPress('Down'),
					keyPress('Down'),
					keyPress('Down'),
					keyPress('Down'),
				],
			},
		},
		{
			checkpoint: 'compact-sessions-details-title',
			actions: [keyPress('Down'), keyPress('Down'), keyPress('Down')],
			actionsByGrid: { '120x36': [] },
		},
		{
			checkpoint: 'compact-sessions-folder-choice',
			actions: [
				keyPress('Escape'),
				visibleText('2\\/6.*Claude Gamma'),
				keyPress('Ctrl+F'),
				keyPress('Tab'),
				keyPress('Enter'),
				typedText('Checkout'),
				keyPress('Enter'),
			],
		},
		{
			checkpoint: 'compact-sessions-model-provider-choice',
			actions: [keyPress('Escape'), keyPress('Tab'), keyPress('Enter')],
		},
		{
			checkpoint: 'compact-sessions-source-choice',
			actions: [keyPress('Escape'), keyPress('Tab'), keyPress('Enter')],
		},
		{
			checkpoint: 'compact-sessions-runtime-choice',
			actions: [keyPress('Escape'), keyPress('Tab'), keyPress('Enter')],
		},
		{
			checkpoint: 'compact-sessions-sort-choice',
			actions: [keyPress('Escape'), keyPress('Tab'), keyPress('Enter')],
		},
	],
	finish: [
		keyPress('Escape'),
		keyPress('Escape'),
		visibleText('2\\/6.*Claude Gamma'),
		keyPress('Ctrl+D'),
	],
} satisfies ScreenJourney;
