import { absentAtBase, s9GridSizes, s9Reference } from '../s9-journey-definition.ts';
import type { S9Journey } from '../s9-journey-definition.ts';
const shared = {
	requirements: ['U5/R9'],
	surface: 'sessions',
	availability: absentAtBase('S6'),
	reference: s9Reference('R3,R9,R13,R14', 'U5'),
	grids: [...s9GridSizes],
	fixtures: [
		'recording preparation stand-in with strict count/request receipt',
		'committed Client/Machine/Directory/Run stable identifiers',
		'a distinct uncommitted dropdown highlight',
	],
	wrongOutcomes: [
		'dropdown highlight committed by Ctrl+Enter',
		'Start invoked twice',
		'Start inaccessible without enhanced keyboard',
		'legacy Enter treated as distinct Ctrl+Enter',
		'missing or misleading key hint',
	],
	admissionInputs: ['S6 recording preparation fixture; pending-S6 never counts as dispatch proof.'],
} satisfies Omit<S9Journey, 'journeyId' | 'keys' | 'steps'>;
export const newDialogJourneys: readonly S9Journey[] = [
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
			keys: ['legacy'] satisfies S9Journey['keys'],
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
					: [{ kind: 'click', target: 'Start' }]) satisfies S9Journey['steps']),
				{
					kind: 'readback',
					port: 'preparation',
					checks: ['legacy-start-fallback', 'compound-start-once'],
				},
			] satisfies S9Journey['steps'],
		}),
	),
];
