import { expect, test } from 'vitest';

import {
	checkAccountCommand,
	checkLoginReturn,
	checkRefreshObservation,
	checkRemovalReturn,
} from './s9-effect-checks.ts';

const target = {
	accountId: 'account-a',
	label: 'Alpha',
	provider: 'codex',
	routerRoot: '/owned/router',
};
const login = {
	capturedTarget: target,
	invocations: [
		{
			argv: ['account', 'login', '--provider', 'codex', '--label', 'Alpha'],
			routerRoot: '/owned/router',
			termiosAtCommand: 'normal',
			alternateScreen: false,
		},
	],
	termiosBefore: 'normal',
	result: { kind: 'succeeded' },
	returnedToQuota: true,
	selectedAccountId: 'account-a',
	statusAfter: 'available',
	message: '',
};
test('login binds the captured account, exact route, restored terminal and fresh return', () => {
	expect(checkLoginReturn(target, login)).toEqual([]);
	expect(
		checkLoginReturn(target, { ...login, capturedTarget: { ...target, accountId: 'account-b' } }),
	).not.toEqual([]);
	expect(
		checkLoginReturn(target, {
			...login,
			invocations: [...login.invocations, ...login.invocations],
		}),
	).not.toEqual([]);
	expect(
		checkLoginReturn(target, {
			...login,
			invocations: [
				{
					...login.invocations[0],
					argv: ['account', 'login', '--label', 'Beta'],
					alternateScreen: true,
				},
			],
		}),
	).not.toEqual([]);
	expect(
		checkLoginReturn(target, {
			...login,
			selectedAccountId: 'account-b',
			statusAfter: 'relogin-required',
		}),
	).not.toEqual([]);
	expect(
		checkLoginReturn(target, {
			...login,
			result: { kind: 'cancelled' },
			message: 'Login cancelled',
		}),
	).toEqual([]);
	expect(checkLoginReturn(target, { ...login, result: { kind: 'cancelled' } })).not.toEqual([]);
	expect(
		checkLoginReturn(target, {
			...login,
			result: { kind: 'failed', message: 'denied' },
			message: 'denied',
		}),
	).toEqual([]);
	expect(
		checkAccountCommand(target, ['account', 'disable', '--account', 'Alpha'], login),
	).not.toEqual([]);
});
test('removal selects former position, last survivor or none using independent fresh IDs', () => {
	for (const [beforeIds, expectedAfterIds, selectedAfter] of [
		[['left', 'account-a', 'right'], ['left', 'right'], 'right'],
		[['left', 'account-a'], ['left'], 'left'],
		[['account-a'], [], null],
	] as const) {
		const observation = {
			target,
			beforeIds,
			expectedAfterIds,
			actualAfterIds: expectedAfterIds,
			selectedAfter,
			message: 'Removed Alpha',
		};
		expect(checkRemovalReturn(observation)).toEqual([]);
		expect(checkRemovalReturn({ ...observation, selectedAfter: 'account-a' })).not.toEqual([]);
		expect(checkRemovalReturn({ ...observation, actualAfterIds: ['account-a'] })).not.toEqual([]);
	}
});
const pending = {
	serviceCalls: 1,
	callsWhileHeld: 1,
	events: [
		'started',
		'duplicate-key-acknowledged',
		'duplicate-menu-acknowledged',
		'released',
		'settled',
	],
	selectedBefore: 'account-a',
	selectedAfter: 'account-a',
	screenStayedOpen: true,
	refreshingVisible: true,
	refreshDisabledReason: 'Refreshing…',
	freshValuesVisible: true,
	openDetailsFresh: true,
};
test('refresh setup failure is taken from the source result and cannot become success', () => {
	const setup = {
		...pending,
		sourceResult: { kind: 'setup-failed', message: 'fixture read refused' },
		accountOutcomes: [],
		result: { kind: 'failed', failed: [], overall: 'fixture read refused' },
		visibleFailureAccountIds: [],
	};
	expect(checkRefreshObservation(setup)).toEqual([]);
	expect(checkRefreshObservation({ ...setup, result: { kind: 'refreshed' } })).not.toEqual([]);
	expect(
		checkRefreshObservation({
			...setup,
			result: { kind: 'failed', failed: [], overall: 'unrelated error' },
		}),
	).not.toEqual([]);
});
test('refresh preserves per-invocation accounts, reasons and causal single dispatch', () => {
	const outcomes = [
		{ accountId: 'account-a', outcome: { kind: 'refreshed' } },
		{ accountId: 'account-b', outcome: { kind: 'failed', reason: 'fixture rejection' } },
	];
	const partial = {
		...pending,
		sourceResult: { kind: 'ok' },
		accountOutcomes: outcomes,
		result: { kind: 'partial', failed: [{ accountId: 'account-b', message: 'fixture rejection' }] },
		visibleFailureAccountIds: ['account-b'],
	};
	expect(checkRefreshObservation(partial)).toEqual([]);
	expect(checkRefreshObservation({ ...partial, result: { kind: 'refreshed' } })).not.toEqual([]);
	expect(
		checkRefreshObservation({
			...partial,
			result: { kind: 'partial', failed: [{ accountId: 'account-b', message: 'older error' }] },
		}),
	).not.toEqual([]);
	expect(checkRefreshObservation({ ...partial, serviceCalls: 2 })).not.toEqual([]);
	expect(
		checkRefreshObservation({
			...partial,
			events: [
				'started',
				'released',
				'duplicate-key-acknowledged',
				'duplicate-menu-acknowledged',
				'settled',
			],
		}),
	).not.toEqual([]);
	expect(checkRefreshObservation({ ...partial, freshValuesVisible: false })).not.toEqual([]);
	const allFailed = {
		...partial,
		sourceResult: { kind: 'all-failed' },
		accountOutcomes: outcomes.slice(1),
		result: {
			kind: 'failed',
			failed: [{ accountId: 'account-b', message: 'fixture rejection' }],
			overall: null,
		},
	};
	expect(checkRefreshObservation(allFailed)).toEqual([]);
	expect(
		checkRefreshObservation({
			...allFailed,
			result: {
				kind: 'partial',
				failed: [{ accountId: 'account-b', message: 'fixture rejection' }],
			},
		}),
	).not.toEqual([]);
	const success = {
		...partial,
		accountOutcomes: outcomes.slice(0, 1),
		result: { kind: 'refreshed' },
		visibleFailureAccountIds: [],
	};
	expect(checkRefreshObservation(success)).toEqual([]);
});
