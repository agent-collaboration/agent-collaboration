import { absentAtBase, flowGridSizes, flowReference } from './flow-definition.ts';
import type { FlowJourney } from './flow-definition.ts';

export const claudeDetailsJourneys: readonly FlowJourney[] = [
	{
		journeyId: 'stopped-claude-details-open-only',
		requirements: ['U12/R13'],
		surface: 'sessions',
		availability: absentAtBase('S6'),
		reference: flowReference(
			'R13; owner 2026-10-30 Open-only correction',
			'U12; U4 captured action identity',
		),
		grids: [...flowGridSizes],
		keys: ['legacy', 'kitty'],
		fixtures: [
			'stopped native Claude session with saved settings and exact source-qualified identity',
			'S6 returned-action receipt from the real loop',
		],
		steps: [
			{
				kind: 'fixture',
				operation: 'select-stopped-native-claude',
				purpose:
					'Distinguish a stopped native session from a managed Claude row and another same-ID source.',
			},
			{ kind: 'press', key: 'Enter' },
			{
				kind: 'capture',
				checkpoint: 'stopped-claude-details',
				checks: ['stopped-claude-open-only'],
			},
			{ kind: 'focus', target: 'Open', via: 'Tab' },
			{ kind: 'press', key: 'Enter' },
			{ kind: 'readback', port: 'source-qualified-outcome', checks: ['captured-session-outcome'] },
		],
		wrongOutcomes: [
			'Resume in background is present',
			'Open is missing',
			'same visible ID resolves to another origin/service/endpoint',
			'a new session replaces the captured stopped session',
		],
		admissionInputs: [
			'S6 test-only returned-action receipt is adopted but not present at base. B proves captured effect identity; actual saved-settings resume and attachment remain the separately granted F native journey.',
		],
	},
];
