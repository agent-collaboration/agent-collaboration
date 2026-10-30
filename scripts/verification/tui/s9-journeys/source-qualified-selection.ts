/** U4 return-path fixtures: HostedCodex, never in-picker HostedProvider actions. */
import { absentAtBase, s9GridSizes, s9Reference } from '../s9-journey-definition.ts';
import type { S9Journey } from '../s9-journey-definition.ts';
export const sourceQualifiedJourneys: readonly S9Journey[] = (['resume', 'fork'] as const).map(
	(action) =>
		({
			journeyId: 'same-id-hosted-codex-' + action,
			requirements: ['U4/R4'],
			surface: 'sessions',
			availability: absentAtBase('S6'),
			reference: s9Reference('R4,R16', 'U4'),
			grids: [...s9GridSizes],
			keys: ['legacy', 'kitty'],
			fixtures: [
				'same opaque ID on HostedCodex rows with distinct services and endpoints',
				'independently declared full expected SessionRef for the selected row',
				'fixture-owned picker-outcome.json written from the real returned selection',
			],
			steps: [
				{
					kind: 'fixture',
					operation: 'select-same-id-hosted-codex',
					purpose:
						'Unmet S6b binding: run selected rows across service/endpoint variants; preserve the full independently declared target.',
				},
				{ kind: 'press', key: 'Enter' },
				{ kind: 'focus', target: action === 'resume' ? 'Open' : 'Fork', via: 'Tab' },
				{ kind: 'press', key: 'Enter' },
				...(action === 'fork'
					? ([
							{ kind: 'focus', target: 'Fork', via: 'Tab' },
							{ kind: 'press', key: 'Enter' },
						] satisfies S9Journey['steps'])
					: []),
				{
					kind: 'readback',
					port: 'source-qualified-outcome',
					checks: ['captured-session-outcome'],
				},
			],
			wrongOutcomes: [
				'same ID but wrong service',
				'same ID but wrong endpoint',
				'HostedProvider instead of HostedCodex',
				'wrong outcome kind',
				'missing target or extra field',
			],
			admissionInputs: [
				'Lead source correction 01a18e4a-bb03-7553-bb56-b784c5ad45e3; S6b real fixture bytes pending.',
				'Provider-row Open remains inside client-action component tests, never claimed as a ResumeSession return.',
			],
		}) satisfies S9Journey,
);
