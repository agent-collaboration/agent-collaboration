import { absentAtBase, s9GridSizes, s9Reference } from '../s9-journey-definition.ts';
import type { S9Journey } from '../s9-journey-definition.ts';
export const toolbarMenuJourneys: readonly S9Journey[] = (['sessions', 'quota'] as const).map(
	(surface) => ({
		journeyId: surface + '-toolbar-and-menu',
		requirements: surface === 'quota' ? ['U12/R13', 'U16/R17', 'U17/R18'] : ['U12/R13'],
		surface,
		availability: absentAtBase(surface === 'sessions' ? 'S6' : 'S7'),
		reference: s9Reference('R6,R7,R8,R13; quota extensions R17/R18', 'U12; quota U16/U17'),
		grids: [...s9GridSizes],
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
						{ kind: 'click', target: 'Foreground' },
						{
							kind: 'capture',
							checkpoint: 'new-foreground-choice',
							checks: ['new-run-mode-by-capability'],
						},
						{ kind: 'press', key: 'Escape' },
						{ kind: 'press', key: '/' },
						{ kind: 'type', text: 'fixture unique session query' },
						{ kind: 'capture', checkpoint: 'live-filter', checks: ['filter-focus-and-live-query'] },
						{ kind: 'press', key: 'Escape' },
					] satisfies S9Journey['steps'])
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
