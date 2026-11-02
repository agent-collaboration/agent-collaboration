/** One hard screen journey; only viewport-specific actions are declared per grid. */
import { keyPress, visibleText, typedText } from '../journey-definition.ts';
import type { ScreenJourney } from '../journey-definition.ts';

export default {
	screen: 'hard',
	groups: [
		{ checkpoint: 'component-quota-hard-browse', grids: ['120x36', '76x30', '32x24'], actions: [] },
		{
			checkpoint: 'component-quota-hard-beta-windows',
			grids: ['120x36', '76x30', '32x24'],
			actions: [keyPress('Down')],
		},
		{ checkpoint: 'component-quota-hard-browse-top', grids: ['48x12'], actions: [] },
		{
			checkpoint: 'component-quota-hard-alpha-name-continued',
			grids: ['48x12'],
			actions: [keyPress('PageDown')],
		},
		{
			checkpoint: 'component-quota-hard-alpha-week',
			grids: ['48x12'],
			actions: [keyPress('PageDown')],
		},
		{
			checkpoint: 'component-quota-hard-alpha-short',
			grids: ['48x12'],
			actions: [keyPress('PageDown')],
		},
		{
			checkpoint: 'component-quota-hard-beta-selected',
			grids: ['48x12'],
			actions: [keyPress('PageUp'), keyPress('PageUp'), keyPress('PageUp'), keyPress('Down')],
		},
		{
			checkpoint: 'component-quota-hard-beta-name-continued',
			grids: ['48x12'],
			actions: [keyPress('PageDown')],
		},
		{
			checkpoint: 'component-quota-hard-beta-week',
			grids: ['48x12'],
			actions: [keyPress('PageDown')],
		},
		{
			checkpoint: 'component-quota-hard-beta-short',
			grids: ['48x12'],
			actions: [keyPress('PageDown')],
		},
		{
			checkpoint: 'component-quota-hard-beta-extra',
			actions: [],
			actionsByGrid: {
				'48x12': [keyPress('PageDown'), keyPress('PageDown'), keyPress('PageDown')],
				'32x24': [keyPress('PageDown')],
			},
		},
		{
			checkpoint: 'component-quota-hard-pool-details',
			actions: [keyPress('F5'), keyPress('Up', 256)],
			actionsByGrid: {
				'48x12': [
					keyPress('PageUp'),
					keyPress('PageUp'),
					keyPress('PageUp'),
					keyPress('PageUp'),
					keyPress('PageUp'),
					keyPress('PageUp'),
					keyPress('F5'),
					keyPress('Up', 256),
				],
				'32x24': [keyPress('PageUp'), keyPress('F5'), keyPress('Up', 256)],
			},
		},
		{ checkpoint: 'component-quota-hard-pool-details-complete', grids: ['120x36'], actions: [] },
		{
			checkpoint: 'component-quota-hard-pool-details-middle',
			actions: [],
			actionsByGrid: {
				'48x12': [keyPress('Down'), keyPress('Down'), keyPress('Down'), keyPress('Down')],
			},
		},
		{ checkpoint: 'component-quota-hard-pool-details-bottom', actions: [keyPress('Down', 256)] },
		{
			checkpoint: 'component-quota-hard-pool-closed',
			grids: ['120x36', '76x30', '32x24'],
			actions: [keyPress('Escape')],
		},
		{
			checkpoint: 'component-quota-hard-pool-closed-top',
			grids: ['48x12'],
			actions: [keyPress('Escape')],
		},
		{ checkpoint: 'component-quota-hard-details', actions: [keyPress('F4'), keyPress('Up', 256)] },
	],
	finish: [keyPress('Escape'), visibleText('Quota status'), typedText('q')],
} satisfies ScreenJourney;
