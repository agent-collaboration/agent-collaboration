import { journeyGridSizes } from '../journey-definition.ts';
import { absentAtBase, flowReference } from './flow-definition.ts';
import type { FlowJourney } from './flow-definition.ts';
const toolbarMenuDrafts: readonly FlowJourney[] = (['sessions', 'quota'] as const).map(
	(surface) => ({
		journeyId: surface + '-toolbar-and-menu',
		requirements: surface === 'quota' ? ['U12/R13', 'U16/R17', 'U17/R18'] : ['U12/R13'],
		surface,
		availability: absentAtBase(surface === 'sessions' ? 'S6' : 'S7'),
		reference: flowReference('R6,R7,R8,R13; quota extensions R17/R18', 'U12; quota U16/U17'),
		grids: [...journeyGridSizes],
		keys: ['legacy', 'kitty'],
		fixtures: [
			'declared counts and current machine',
			'independent wide toolbar reference',
			'closed action inventory with base Help retention',
		],
		steps: [
			{
				kind: 'capture',
				checkpoint: 'main-top-row',
				checks: [surface === 'sessions' ? 'picker-toolbar' : 'quota-toolbar', 'no-bottom-actions'],
			},
			{ kind: 'press', key: 'F10' },
			{
				kind: 'capture',
				checkpoint: 'menu-f10',
				checks: ['closed-menu-inventory', 'menu-key-and-tab', 'projection-parity'],
			},
			{ kind: 'press', key: 'Escape' },
			{ kind: 'click', target: '≡ Menu' },
			{
				kind: 'capture',
				checkpoint: 'menu-from-click',
				checks:
					surface === 'sessions'
						? ['closed-menu-inventory', 'checked-settings']
						: ['closed-menu-inventory'],
			},
			{ kind: 'press', key: 'Escape' },
			{ kind: 'press', key: 'F1' },
			{
				kind: 'capture',
				checkpoint: 'help-actions-settings-conventions',
				checks:
					surface === 'sessions'
						? ['projection-parity', 'help-conventions-and-menu-control']
						: ['projection-parity'],
			},
			{ kind: 'press', key: 'Escape' },
			{ kind: 'focus', target: 'main list', via: 'Shift+Tab' },
			{ kind: 'focus', target: '≡ Menu', via: 'Tab' },
			{ kind: 'press', key: 'Enter' },
			{
				kind: 'capture',
				checkpoint: 'menu-from-tab',
				checks: ['closed-menu-inventory', 'menu-key-and-tab'],
			},
			{ kind: 'press', key: 'Escape' },
			...(surface === 'sessions'
				? ([
						{
							kind: 'fixture',
							operation: 'new-client-native-claude-with-both-run-modes',
							purpose: 'Declare background-New availability independently; no provider launch.',
						},
						{ kind: 'press', key: 'F10' },
						{ kind: 'click', target: 'New background session…' },
						{
							kind: 'capture',
							checkpoint: 'new-background-preset',
							checks: ['new-run-mode-by-capability'],
						},
						{ kind: 'focus', target: 'Run', via: 'Tab' },
						{ kind: 'click', target: 'Run' },
						{ kind: 'click', target: 'Foreground' },
						{
							kind: 'capture',
							checkpoint: 'new-foreground-choice',
							checks: ['new-run-mode-by-capability'],
						},
						{ kind: 'press', key: 'Escape' },
						{ kind: 'focus', target: 'main list', via: 'Tab' },
						{ kind: 'press', key: '/' },
						{
							kind: 'capture',
							checkpoint: 'filter-shortcut-focus',
							checks: ['filter-focus-and-live-query'],
						},
						{ kind: 'focus', target: 'main list', via: 'Tab' },
						{ kind: 'click', target: 'Filter' },
						{
							kind: 'capture',
							checkpoint: 'filter-click-focus',
							checks: ['filter-focus-and-live-query'],
						},
						{ kind: 'type', text: '0123456789A' },
						{ kind: 'capture', checkpoint: 'live-filter', checks: ['filter-focus-and-live-query'] },
						{ kind: 'press', key: 'Escape' },
					] satisfies FlowJourney['steps'])
				: []),
			{
				kind: 'fixture',
				operation: 'resize-same-child-to-each-declared-grid',
				purpose:
					'Compare visible button prefixes with the wide reference; counts and right-hand identity stay visible.',
			},
			{
				kind: 'capture',
				checkpoint: 'narrow-top-row',
				checks: ['right-dropoff', 'no-bottom-actions'],
			},
			{ kind: 'press', key: 'F10' },
			{
				kind: 'capture',
				checkpoint: 'narrow-menu-complete',
				checks: ['closed-menu-inventory', 'right-dropoff', 'projection-parity'],
			},
			{ kind: 'press', key: 'Escape' },
		],
		wrongOutcomes: [
			'Help or Quit on toolbar',
			'bottom shortcut/action row',
			'missing dropped action in menu',
			'extra or missing closed-set action',
			'menu key printed but unbound',
			'scope/filter/count/machine lost when narrow',
		],
		admissionInputs: [],
	}),
);

export const toolbarMenuJourneys: readonly FlowJourney[] = toolbarMenuDrafts.map((journey) => {
	if (journey.surface === 'quota')
		return Object.assign({}, journey, {
			steps: journey.steps.flatMap((step): FlowJourney['steps'] => {
				if (step.kind === 'capture' && step.checks.includes('closed-menu-inventory'))
					return [
						{
							kind: 'fixture',
							operation: 'inspect-quota-menu-' + step.checkpoint,
							purpose:
								'Traverse the real closed quota menu and its keys before the original capture.',
						},
						step,
					];
				if (step.kind === 'capture' && step.checkpoint === 'help-actions-settings-conventions')
					return [
						{
							kind: 'fixture',
							operation: 'inspect-quota-menu-help',
							purpose:
								'Collect every physical Help row against the independent action and convention census.',
						},
						step,
					];
				return [step];
			}),
		});
	return Object.assign({}, journey, {
		inputProfile: 'example-ghostty-keybinds',
		steps: journey.steps.flatMap((step): FlowJourney['steps'] => {
			if (step.kind === 'capture' && step.checks.includes('closed-menu-inventory'))
				return [
					{
						kind: 'fixture',
						operation: 'inspect-picker-menu-' + step.checkpoint,
						purpose:
							'Traverse the entire real menu, including checked values and nonfocusable section keys, before its original capture.',
					},
					step,
				];
			if (step.kind === 'capture' && step.checkpoint === 'help-actions-settings-conventions')
				return [
					{
						kind: 'fixture',
						operation: 'inspect-picker-menu-help',
						purpose:
							'Check every physical Help row against the independently retained base conventions and current action/settings census.',
					},
					step,
				];
			if (step.kind === 'click' && step.target === 'New background session…')
				return [
					{
						kind: 'fixture',
						operation: 'reveal-picker-background-new',
						purpose:
							'Reveal the actual menu action through acknowledged focus navigation before clicking its painted cells.',
					},
					step,
				];
			return [step];
		}),
	});
});
