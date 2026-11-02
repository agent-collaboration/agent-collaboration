/** R19 native projection and remembered preference; one aggregate case uses fresh processes. */
import { journeyGridSizes } from '../journey-definition.ts';
import { absentAtBase, flowReference } from './flow-definition.ts';
import type { FlowJourney, FlowStep } from './flow-definition.ts';
import type { PickerGroupingFacet } from './picker-grouping-oracle.ts';

function inspect(facet: PickerGroupingFacet, checkpoint: string): FlowStep[] {
	return [
		{
			kind: 'fixture',
			operation: 'inspect-session-grouping-' + facet,
			purpose:
				'Traverse the nine literal rows and every group heading; preserve selection and check the real menu choice.',
		},
		{
			kind: 'capture',
			checkpoint,
			checks: [
				'session-grouping-buckets',
				'session-grouping-selection',
				'session-grouping-checked',
			],
		},
	];
}

function cycleGrouping(): FlowStep[] {
	// Each press retains its own ACK and marked frame. The first regroup frame is
	// owner-deferred measurement evidence; assertions use the separately marked F12.
	return [
		{ kind: 'press', key: 'F6' },
		{ kind: 'press', key: 'F12' },
	];
}
function groupingJourney(journeyId: string, steps: FlowStep[]): FlowJourney {
	return {
		journeyId,
		requirements: ['U18/R19'],
		surface: 'sessions',
		availability: absentAtBase('S6'),
		reference: flowReference(
			'R19; fixed nine-row metadata and mounted fixture clock',
			'V19; same store reopened through fresh real picker processes',
		),
		grids: [...journeyGridSizes],
		keys: ['legacy', 'kitty'],
		inputProfile: 'example-ghostty-keybinds',
		fixtures: [
			'r19-grouping literal source-qualified identities',
			'private per-case preferences directory under lane tmp',
		],
		steps,
		wrongOutcomes: [
			'wrong bucket or count',
			'heading selected',
			'row identity/order lost',
			'selection changed by grouping',
			'None differs from ungrouped order',
			'forgotten grouping or error over damaged stored preference',
		],
		admissionInputs: [
			'Declared signed product/driver and independent literal bucket/order oracle.',
			'Aggregate persistence runner owns fresh private paths, real first exit/drain, reopen and malformed-value controls.',
		],
	};
}
export const pickerGroupingJourney = groupingJourney('sessions-group-by-f6', [
	...inspect('none', 'grouping-none-initial'),
	...(['started-by', 'day', 'folder', 'provider', 'machine'] as const).flatMap(
		(facet): FlowStep[] => {
			return cycleGrouping().concat(inspect(facet, 'grouping-' + facet));
		},
	),
	...cycleGrouping(),
	...inspect('none', 'grouping-none-restored'),
	...Array.from({ length: 4 }, cycleGrouping).flat(),
	...inspect('provider', 'grouping-provider-before-close'),
]);
// These phases are internal to the aggregate runner, never independently admitted as fresh-empty passes.
export const pickerGroupingReopenJourney = groupingJourney(
	'sessions-group-by-reopened',
	inspect('provider', 'grouping-reopened-provider'),
);
export const pickerGroupingFallbackJourney = groupingJourney(
	'sessions-group-by-fallback',
	inspect('none', 'grouping-fallback-none'),
);
