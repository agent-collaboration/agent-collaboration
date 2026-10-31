import { journeyGridSizes } from '../journey-definition.ts';
import { absentAtBase, flowReference } from './flow-definition.ts';
import type { FlowJourney } from './flow-definition.ts';
const shared = {
	requirements: ['U5/R9'],
	surface: 'sessions',
	availability: absentAtBase('S6'),
	reference: flowReference('R3,R9,R13,R14', 'U5'),
	grids: [...journeyGridSizes],
	fixtures: [
		'returned New outcome for local/default; counted preparation stand-in only for configured source',
		'committed Client/Machine/Directory/Run stable identifiers',
		'a distinct uncommitted dropdown highlight',
	],
	wrongOutcomes: [
		'dropdown highlight committed by Ctrl+Enter',
		'Start invoked twice',
		'wrong returned client or run mode',
		'missing or duplicate returned outcome',
		'Start inaccessible without enhanced keyboard',
		'legacy Enter treated as distinct Ctrl+Enter',
		'missing or misleading key hint',
	],
	admissionInputs: [
		'S6 returned local/default New outcome; configured source alone uses counted preparer. Lead wire decision 01a18e37-0d51-78d2-b22c-01996654eb1d; pending-S6 is never proof.',
	],
} satisfies Omit<FlowJourney, 'journeyId' | 'keys' | 'steps'>;
const activationJourneys: readonly FlowJourney[] = [
	{
		...shared,
		journeyId: 'new-dropdown-compound-start',
		keys: ['kitty'],
		steps: [
			{ kind: 'press', key: 'Ctrl+N' },
			{ kind: 'focus', target: 'Machine', via: 'arrows' },
			{ kind: 'press', key: 'Enter' },
			{ kind: 'press', key: 'Down' },
			{
				kind: 'capture',
				checkpoint: 'dropdown-highlight',
				checks: ['dropdown-highlight-uncommitted'],
			},
			{ kind: 'press', key: 'Ctrl+Enter' },
			{ kind: 'readback', port: 'preparation', checks: ['compound-start-once'] },
		],
	},
	...(['tab-enter', 'click'] as const).map((activation) =>
		Object.assign({}, shared, {
			journeyId: 'new-legacy-start-' + activation,
			keys: ['legacy'] satisfies FlowJourney['keys'],
			steps: [
				{ kind: 'press', key: 'Ctrl+N' },
				{ kind: 'focus', target: 'Machine', via: 'Tab' },
				{ kind: 'press', key: 'Enter' },
				{ kind: 'press', key: 'Down' },
				{
					kind: 'capture',
					checkpoint: 'legacy-dropdown-open',
					checks: ['dropdown-highlight-uncommitted'],
				},
				{ kind: 'press', key: 'Escape' },
				...((activation === 'tab-enter'
					? [
							{ kind: 'focus', target: 'Start', via: 'Tab' },
							{ kind: 'press', key: 'Enter' },
						]
					: [{ kind: 'click', target: 'Start' }]) satisfies FlowJourney['steps']),
				{
					kind: 'readback',
					port: 'preparation',
					checks: ['legacy-start-fallback', 'compound-start-once'],
				},
			] satisfies FlowJourney['steps'],
		}),
	),
];

const sourceCases = [
	{
		name: 'default',
		machine: 'defaultHosted',
		port: 'source-qualified-outcome',
		check: 'returned-new-once',
	},
	{
		name: 'local',
		machine: 'localCodex',
		port: 'source-qualified-outcome',
		check: 'returned-new-once',
	},
	{
		name: 'configured',
		machine: 'configured:fixture',
		port: 'preparation',
		check: 'configured-start-once',
	},
] as const;
export const newDialogJourneys: readonly FlowJourney[] = sourceCases.flatMap((source) =>
	activationJourneys.map((journey) =>
		Object.assign({}, journey, {
			journeyId: journey.journeyId + '-' + source.name,
			fixtures: [...journey.fixtures, 'independently declared source ' + source.machine],
			steps: [
				{
					kind: 'fixture',
					operation: 'select-New-source:' + source.machine,
					purpose:
						'Unmet S6 binding requirement: drive the named source and retain its committed fields; not an implemented hook.',
				},
				...journey.steps.map((step) =>
					step.kind === 'readback'
						? Object.assign({}, step, { port: source.port, checks: [...step.checks, source.check] })
						: step,
				),
			],
		}),
	),
);
