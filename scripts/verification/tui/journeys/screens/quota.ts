/** One quota screen journey; only viewport-specific actions are declared per grid. */
import { visibleText, keyPress, typedText, dialogField } from '../journey-definition.ts';
import type { ScreenJourney } from '../journey-definition.ts';

export default {
	screen: 'quota',
	groups: [
		{
			checkpoint: 'compact-quota-all',
			actions: [
				visibleText('All.*Disabled: Shown.*4\\/4( accounts| ▾)'),
				keyPress('Up'),
				keyPress('Up'),
				keyPress('Up'),
			],
		},
		{ checkpoint: 'compact-quota-pool-details', actions: [keyPress('F5'), keyPress('Up', 256)] },
		{ checkpoint: 'compact-quota-pool-details-complete', grids: ['120x36'], actions: [] },
		{
			checkpoint: 'compact-quota-pool-details-middle',
			actions: [],
			actionsByGrid: { '48x12': [keyPress('Down'), keyPress('Down'), keyPress('Down')] },
		},
		{ checkpoint: 'compact-quota-pool-details-bottom', actions: [keyPress('Down', 256)] },
		{ checkpoint: 'compact-quota-pool-closed', actions: [keyPress('Escape')] },
		{
			checkpoint: 'compact-quota-filters-block-pool',
			actions: [
				keyPress('Ctrl+F'),
				visibleText('Account filters.*Account provider'),
				keyPress('F5'),
			],
		},
		{
			checkpoint: 'compact-quota-provider-no-match',
			actions: [
				keyPress('Escape'),
				visibleText('All.*Disabled: Shown.*4\\/4( accounts| ▾).*Claude Delta'),
				keyPress('Ctrl+F'),
				keyPress('Enter'),
				typedText('zznomatchzz'),
			],
		},
		{
			checkpoint: 'compact-quota-chooser',
			actions: [
				keyPress('Escape'),
				keyPress('Enter'),
				keyPress('Down'),
				keyPress('Enter'),
				keyPress('Down'),
			],
		},
		{
			checkpoint: 'compact-quota-help',
			actions: [keyPress('Up'), keyPress('F1')],
			actionsByGrid: {
				'48x12': [keyPress('Up'), keyPress('F1'), visibleText('Quota Help'), keyPress('Down', 8)],
				'32x24': [keyPress('Up'), keyPress('F1'), visibleText('Quota Help'), keyPress('Down', 5)],
			},
		},
		{
			checkpoint: 'compact-quota-disabled-choice',
			actions: [
				keyPress('Escape'),
				dialogField('Account filters', 'Account provider', 'OpenAI'),
				keyPress('Enter'),
				keyPress('Down'),
				keyPress('Enter'),
				keyPress('Tab'),
				keyPress('Enter'),
			],
		},
		{ checkpoint: 'compact-quota-hidden-choice', actions: [keyPress('Down')] },
		{
			checkpoint: 'compact-quota-claude',
			actions: [keyPress('Enter'), keyPress('Tab'), keyPress('Enter')],
		},
		{
			checkpoint: 'compact-quota-cancelled',
			actions: [
				keyPress('Ctrl+F'),
				keyPress('Enter'),
				keyPress('Up'),
				keyPress('Enter'),
				dialogField('Account filters', 'Account provider', 'OpenAI'),
				keyPress('Escape'),
			],
		},
		{ checkpoint: 'compact-quota-details', actions: [keyPress('F4')] },
	],
	finish: [
		keyPress('Escape'),
		visibleText('Claude.*Disabled: Hidden.*1\\/4( accounts| ▾).*Claude Gamma'),
		typedText('q'),
	],
} satisfies ScreenJourney;
