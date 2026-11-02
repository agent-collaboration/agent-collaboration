/** R19's two real CLI invocations, exact saves and remount, on one retained PTY. */
import { journeyGridSizes } from '../journey-definition.ts';
import { absentAtBase, flowReference } from './flow-definition.ts';
import type { FlowJourney, FlowStep } from './flow-definition.ts';
function inspect(facet: 'none' | 'provider' | 'status', checkpoint: string): FlowStep[] {
	return [
		{
			kind: 'fixture',
			operation: 'inspect-quota-grouping-' + facet,
			purpose:
				'Read both literal accounts and every physical group heading/count; inspect the actual checked menu item, preserving selection.',
		},
		{
			kind: 'capture',
			checkpoint,
			checks: ['quota-grouping-buckets', 'quota-grouping-selection', 'quota-grouping-checked'],
		},
	];
}
export const quotaGroupingJourney: FlowJourney = {
	journeyId: 'quota-group-by-f6',
	requirements: ['U18/R19'],
	surface: 'quota',
	availability: absentAtBase('S7'),
	reference: flowReference(
		'R19; declared grouping-reopen fixture, same store reopened by actual CLI',
		'V19; None→Provider, guarded Quit, reopened Provider→Status; exact saves and final persisted status',
	),
	grids: [...journeyGridSizes],
	keys: ['legacy', 'kitty'],
	inputProfile: 'example-ghostty-keybinds',
	fixtures: [
		'two source-declared distinct account identities/providers',
		'real private UI store and existing child guard, with no login child',
	],
	steps: [
		...inspect('none', 'quota-grouping-none'),
		{ kind: 'press', key: 'F6' },
		{ kind: 'press', key: 'F12' },
		...inspect('provider', 'quota-grouping-provider'),
		{ kind: 'press', key: 'Ctrl+C' },
		...inspect('provider', 'quota-grouping-reopened-provider'),
		{ kind: 'press', key: 'F6' },
		{ kind: 'press', key: 'F12' },
		...inspect('status', 'quota-grouping-status'),
		{ kind: 'press', key: 'Ctrl+C' },
		{
			kind: 'readback',
			port: 'quota-preferences',
			checks: ['quota-grouping-persistence', 'no-retry'],
		},
	],
	wrongOutcomes: [
		'selection changes during grouping',
		'missing or wrong buckets/counts',
		'checked menu differs from choice',
		'first Quit has no actual preference drain/remount',
		'wrong store/mount, dropped or duplicated preference save',
		'a login child is substituted for invocation reopen',
	],
	admissionInputs: [
		'S7 declared effectful libtest and private guard stand-in; exact two F6 and two guarded Quit actions',
		'Actual first settlement and next mount via await_event fence; separate F12 for each adopted regroup frame',
	],
};
