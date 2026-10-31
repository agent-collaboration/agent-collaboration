import { absentAtBase, s9Reference } from '../s9-journey-definition.ts';
import type { S9Journey } from '../s9-journey-definition.ts';
export const accountLoginJourneys: readonly S9Journey[] = (
	['succeeded', 'failed', 'cancelled-130'] as const
).map((outcome) => ({
	journeyId: 'quota-login-' + outcome,
	requirements: ['U16/R17'],
	surface: 'quota',
	availability: absentAtBase('S7'),
	reference: s9Reference(
		'R17; Program Design Account actions from quota Details',
		'U16: invocation, fresh data, failure, cancel, captured target',
	),
	grids: ['120x36'],
	keys: ['legacy', 'kitty'],
	fixtures: [
		'at least two distinct account IDs/providers/labels',
		'recording account-command stand-in on scoped PATH, no OAuth',
		'termios/alternate-screen readback at actual command entry',
		'changed fixture report on successful login',
	],
	steps: [
		{
			kind: 'fixture',
			operation: 'select-relogin-required-account-a',
			purpose:
				'Capture expected AccountId/provider/label and Router root independently of browse selection.',
		},
		{ kind: 'press', key: 'Enter' },
		{
			kind: 'capture',
			checkpoint: 'account-details-login',
			checks: ['login-primary-by-status', 'exact-login-command'],
		},
		{
			kind: 'fixture',
			operation: 'move-browse-to-b-and-reload',
			purpose: 'Details must keep captured A; changing browse cannot retarget any action.',
		},
		{
			kind: 'fixture',
			operation: 'login-command-' + outcome,
			purpose: 'Own the one command result; success changes A from relogin-required to available.',
		},
		{ kind: 'click', target: 'Log in again' },
		{
			kind: 'readback',
			port: 'account-command',
			checks: [
				'captured-account-target',
				'exact-login-command',
				'command-after-terminal-restore',
				'no-retry',
			],
		},
		{
			kind: 'capture',
			checkpoint: 'quota-return-' + outcome,
			checks: ['command-result-and-fresh-return', 'captured-account-target'],
		},
		...(outcome === 'succeeded'
			? ([
					{ kind: 'press', key: 'Enter' },
					{
						kind: 'capture',
						checkpoint: 'available-details-primary-close',
						checks: ['login-primary-by-status'],
					},
				] satisfies S9Journey['steps'])
			: []),
	],
	wrongOutcomes: [
		'wrong provider/label/root',
		'command inside quota terminal modes',
		'duplicate command',
		'stale report after successful login',
		'failure/cancel exits quota',
		'failure/cancel silently retries',
		'captured A retargeted to browse B',
	],
	admissionInputs: [
		'S7 CLI account-command recording fixture and typed result readback; login itself has no extra confirmation specified by R17. Remove confirmation is authored separately.',
	],
}));
