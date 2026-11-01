import { expect, test } from 'vitest';

import {
	checkQuotaEffectReceipt,
	requireQuotaLoginParentExit,
	checkQuotaRefreshReceipt,
	quotaEffectReceiptSchema,
} from './quota-effect-readback.ts';
import { quotaEffectConfigurationSchema, quotaJourneyScenarios } from './quota-fixture-contract.ts';

function cancelReceipt(): unknown {
	const accounts = [
		{
			accountId: 'effect-account-0',
			provider: 'openai',
			label: 'effect-alpha',
			status: 'disabled',
			credentialGeneration: 7,
		},
		{
			accountId: 'effect-account-1',
			provider: 'openai',
			label: 'effect-beta',
			status: 'enabled',
			credentialGeneration: 7,
		},
	];
	return {
		scenario: 'remove-cancel',
		outcomes: [{ kind: 'quit' }],
		before: accounts,
		after: accounts,
		policiesBefore: {
			floors: [],
			credits: [
				{ accountId: 'effect-account-0', policy: 'fixture-default' },
				{ accountId: 'effect-account-1', policy: 'fixture-default' },
			],
		},
		policiesAfter: {
			floors: [],
			credits: [
				{ accountId: 'effect-account-0', policy: 'fixture-default' },
				{ accountId: 'effect-account-1', policy: 'fixture-default' },
			],
		},
		accountCommands: [],
		clipboard: [],
		removalGateEntered: false,
		removalGateReleased: false,
		refresh: null,
		loginChild: null,
		loginParentExit: null,
		resetRequests: [],
		fixtureMutations: [],
		unexpectedEffectCalls: [],
		publishedPoses: [
			{
				mount: 1,
				browseAccountId: 'effect-account-0',
				listSelection: 'effect-account-0',
				captured: null,
				rows: [],
			},
		],
		nativeDispatch: [
			{
				sequence: 1,
				event: { kind: 'key', code: { kind: 'fixed', name: 'F12' }, modifiers: [], phase: 'press' },
				surface: { kind: 'screen' },
				target: null,
				action: null,
			},
		],
	};
}
test('raw cancelled removal accepts zero effects and rejects actual removal or wrong seed', () => {
	expect(
		checkQuotaEffectReceipt({
			scenario: 'remove-cancel',
			routerRoot: '/fixture',
			value: cancelReceipt(),
		}),
	).toEqual([]);
	const changed = quotaEffectReceiptSchema.parse(cancelReceipt());
	changed.after = changed.after.slice(1);
	expect(
		checkQuotaEffectReceipt({ scenario: 'remove-cancel', routerRoot: '/fixture', value: changed }),
	).toContain('persisted account effects differ from exact scenario');
	const duplicate = quotaEffectReceiptSchema.parse(cancelReceipt());
	duplicate.accountCommands.push({
		command: {
			accountId: 'effect-account-1',
			label: 'effect-beta',
			provider: 'openai',
			position: 1,
			action: 'remove',
		},
		result: { kind: 'succeeded' },
	});
	expect(
		checkQuotaEffectReceipt({
			scenario: 'remove-cancel',
			routerRoot: '/fixture',
			value: duplicate,
		}),
	).toContain('command set retargeted captured A, repeated or invoked an unrelated action');
});
test('a setup acknowledgement can never substitute for native dispatch', () => {
	const value = {
		...quotaEffectReceiptSchema.parse(cancelReceipt()),
		nativeDispatch: [
			{
				kind: 'fixture-control-ack',
				requestId: 1,
				operationId: 1,
				operation: 'reload-view-model',
				readback: {},
			},
		],
	};
	expect(() =>
		checkQuotaEffectReceipt({ scenario: 'remove-cancel', routerRoot: '/fixture', value }),
	).toThrow();
});

test('raw reader rejects deletion of an unrelated account credit policy', () => {
	const value = quotaEffectReceiptSchema.parse(cancelReceipt());
	value.policiesAfter.credits = value.policiesAfter.credits.slice(0, 1);
	expect(
		checkQuotaEffectReceipt({ scenario: 'remove-cancel', routerRoot: '/fixture', value }),
	).toContain('unrelated policy, reset or effect-port mutation');
});
test('raw reader rejects an accepted unrelated policy action even when its persisted value stayed equal', () => {
	const value = quotaEffectReceiptSchema.parse(cancelReceipt());
	value.nativeDispatch.push({
		sequence: 2,
		event: { kind: 'key', code: { kind: 'fixed', name: 'Return' }, phase: 'press', modifiers: [] },
		surface: { kind: 'overlay', overlayId: '1' },
		target: 'action:quota:save-credit',
		action: 'quota:save-credit',
	});
	expect(
		checkQuotaEffectReceipt({ scenario: 'remove-cancel', routerRoot: '/fixture', value }),
	).toContain('accepted an unrelated policy or reset owner action');
});
function partialReceipt(): unknown {
	const source = {
		kind: 'ok',
		message: null,
		accountOutcomes: [
			{ accountId: 'effect-account-0', outcome: { kind: 'refreshed' } },
			{ accountId: 'effect-account-1', outcome: { kind: 'failed', reason: 'actual beta 429' } },
		],
	};
	return {
		serviceCalls: 1,
		callsWhileHeld: 1,
		released: true,
		observations: [
			{ kind: 'service-returned', source },
			{ kind: 'mapping-consumed', source },
			{
				kind: 'mapping-returned',
				result: {
					kind: 'partial',
					failed: [{ accountId: 'effect-account-1', message: 'actual beta 429' }],
				},
			},
		],
		requests: Array.from({ length: 6 }, () => ({ method: 'GET' })),
	};
}
test('real service Ok plus failed beta must map to Partial with the exact reason', () => {
	expect(checkQuotaRefreshReceipt('refresh-partial', partialReceipt())).toEqual([]);
	const value = partialReceipt();
	if (typeof value !== 'object' || value === null) throw new Error('fixture absent');
	const wrong = {
		...value,
		observations: [
			{
				kind: 'service-returned',
				source: {
					kind: 'ok',
					message: null,
					accountOutcomes: [
						{ accountId: 'effect-account-0', outcome: { kind: 'refreshed' } },
						{
							accountId: 'effect-account-1',
							outcome: { kind: 'failed', reason: 'actual beta 429' },
						},
					],
				},
			},
			{
				kind: 'mapping-consumed',
				source: {
					kind: 'ok',
					message: null,
					accountOutcomes: [
						{ accountId: 'effect-account-0', outcome: { kind: 'refreshed' } },
						{
							accountId: 'effect-account-1',
							outcome: { kind: 'failed', reason: 'actual beta 429' },
						},
					],
				},
			},
			{ kind: 'mapping-returned', result: { kind: 'refreshed' } },
		],
	};
	expect(checkQuotaRefreshReceipt('refresh-partial', wrong)).toContain(
		'mapper kind differs from declared outcome',
	);
});
test('refresh rejects duplicate invocations and invented setup accounts', () => {
	const source = partialReceipt();
	if (typeof source !== 'object' || source === null) throw new Error('fixture absent');
	expect(checkQuotaRefreshReceipt('refresh-partial', { ...source, serviceCalls: 2 })).toContain(
		'refresh must settle one causally held invocation',
	);
	const setup = {
		serviceCalls: 1,
		callsWhileHeld: 1,
		released: true,
		observations: [
			{ kind: 'setup-failed', message: 'original setup failure' },
			{
				kind: 'mapping-returned',
				result: { kind: 'failed', failed: [], overall: 'original setup failure' },
			},
		],
		requests: [],
	};
	expect(checkQuotaRefreshReceipt('refresh-setup-failed', setup)).toEqual([]);
	expect(
		checkQuotaRefreshReceipt('refresh-setup-failed', {
			...setup,
			observations: [
				{ kind: 'setup-failed', message: 'original setup failure' },
				{
					kind: 'mapping-returned',
					result: {
						kind: 'failed',
						failed: [{ accountId: 'invented', message: 'fake' }],
						overall: 'original setup failure',
					},
				},
			],
		}),
	).toContain('setup error lost its original message or invented attempted accounts');
});
test('login-only configuration fields cannot leak into other effect scenarios', () => {
	const common = {
		scenario: 'remove-cancel',
		outputDirectory: '/private/tmp/quota',
		events: '/private/tmp/quota/events',
		receipt: '/private/tmp/quota/receipt',
		controlSocket: '/private/tmp/quota/control',
		dispatchReceipt: '/private/tmp/quota/dispatch',
	};
	expect(quotaEffectConfigurationSchema.safeParse(common).success).toBe(true);
	expect(
		quotaEffectConfigurationSchema.safeParse({ ...common, loginStandin: '/bin/echo' }).success,
	).toBe(false);
	expect(
		quotaEffectConfigurationSchema.safeParse({ ...common, scenario: 'login-succeeded' }).success,
	).toBe(false);
});
test('one refresh journey requires two fresh child bindings', () => {
	expect(quotaJourneyScenarios('quota-refresh-partial')).toEqual([
		'refresh-partial',
		'refresh-partial',
	]);
	expect(quotaJourneyScenarios('quota-remove-middle')).toEqual(['remove-middle', 'remove-held']);
	expect(quotaJourneyScenarios('quota-group-by-f6')).toEqual([]);
});

test('actual login receipt must retain A, restored terminal, exact exit and a fresh mount', () => {
	const receipt = quotaEffectReceiptSchema.parse(cancelReceipt());
	receipt.scenario = 'login-succeeded';
	receipt.before = [
		{
			accountId: 'effect-account-0',
			provider: 'openai',
			label: 'effect-alpha',
			status: 'enabled',
			credentialGeneration: 7,
		},
		{
			accountId: 'effect-account-1',
			provider: 'claude',
			label: 'effect-beta',
			status: 'enabled',
			credentialGeneration: 7,
		},
	];
	receipt.after = [
		{
			accountId: 'effect-account-0',
			provider: 'openai',
			label: 'effect-alpha',
			status: 'enabled',
			credentialGeneration: 8,
		},
		{
			accountId: 'effect-account-1',
			provider: 'claude',
			label: 'effect-beta',
			status: 'enabled',
			credentialGeneration: 7,
		},
	];
	receipt.outcomes = [
		{
			kind: 'login-again',
			accountId: 'effect-account-0',
			provider: 'openai',
			label: 'effect-alpha',
		},
		{ kind: 'quit' },
	];
	receipt.fixtureMutations = [
		{ kind: 'fixture-mutation', operation: 'activate-fresh-login-generation' },
	];
	receipt.loginParentExit = {
		childPid: 42,
		waitOrdinal: 1,
		status: { kind: 'exited', code: 0, rawStatus: 0 },
	};
	receipt.loginChild = {
		arguments: [
			'account',
			'login',
			'--provider',
			'openai',
			'--label',
			'effect-alpha',
			'--router-root',
			'/fixture',
		],
		exitCode: 0,
		terminal: { kind: 'interactive', canonical: true, echo: true, signals: true, foreground: true },
	};
	receipt.publishedPoses = [
		{
			mount: 1,
			browseAccountId: 'effect-account-1',
			listSelection: 'effect-account-1',
			captured: {
				kind: 'details',
				accountId: 'effect-account-0',
				provider: 'openai',
				label: 'effect-alpha',
				position: 0,
			},
			rows: [],
		},
		{
			mount: 2,
			browseAccountId: 'effect-account-0',
			listSelection: 'effect-account-0',
			captured: null,
			rows: [{ accountId: 'effect-account-0', loginState: 'NotRequired' }],
		},
	];
	receipt.nativeDispatch = [
		{
			sequence: 1,
			event: {
				kind: 'key',
				code: { kind: 'fixed', name: 'Return' },
				phase: 'press',
				modifiers: [],
			},
			surface: { kind: 'overlay', overlayId: '1' },
			target: 'action:quota:login',
			action: 'quota:login',
		},
	];
	expect(
		checkQuotaEffectReceipt({
			scenario: 'login-succeeded',
			routerRoot: '/fixture',
			value: receipt,
		}),
	).toEqual([]);
	const actualParent = receipt.loginParentExit;
	receipt.loginParentExit = null;
	expect(
		checkQuotaEffectReceipt({
			scenario: 'login-succeeded',
			routerRoot: '/fixture',
			value: receipt,
		}),
	).toContain(
		'actual parent login wait identity, count or raw status differs from declared outcome',
	);
	receipt.loginParentExit = actualParent;
	receipt.loginChild.arguments[5] = 'effect-beta';
	expect(
		checkQuotaEffectReceipt({
			scenario: 'login-succeeded',
			routerRoot: '/fixture',
			value: receipt,
		}),
	).toContain('login child argv or planned exit differs from captured target');
	receipt.loginChild.terminal = {
		kind: 'interactive',
		canonical: false,
		echo: true,
		signals: true,
		foreground: true,
	};
	expect(
		checkQuotaEffectReceipt({
			scenario: 'login-succeeded',
			routerRoot: '/fixture',
			value: receipt,
		}),
	).toContain('login child entered before terminal restoration');
	receipt.publishedPoses[1] = {
		mount: 1,
		browseAccountId: 'effect-account-0',
		listSelection: 'effect-account-0',
		captured: null,
		rows: [],
	};
	expect(
		checkQuotaEffectReceipt({
			scenario: 'login-succeeded',
			routerRoot: '/fixture',
			value: receipt,
		}),
	).toContain('actual CLI restart mount absent');
});

for (const code of [0, 130, 7] as const)
	test('parent wait retains the real raw exit ' + code, () => {
		expect(
			requireQuotaLoginParentExit(
				{ childPid: 42, waitOrdinal: 1, status: { kind: 'exited', code, rawStatus: code << 8 } },
				code,
			),
		).toEqual({ kind: 'exited', code });
	});
test('parent wait rejects child intent, absent identity, duplicate wait and fabricated raw status', () => {
	const valid = {
		childPid: 42,
		waitOrdinal: 1,
		status: { kind: 'exited', code: 7, rawStatus: 7 << 8 },
	};
	for (const invalid of [
		null,
		{ exitCode: 7 },
		{ ...valid, childPid: null },
		{ ...valid, childPid: 0 },
		{ ...valid, waitOrdinal: 2 },
		{ ...valid, status: { kind: 'exited', code: 7, rawStatus: 0 } },
		{ ...valid, status: { kind: 'signaled', signal: 2, rawStatus: 2 } },
		{ ...valid, status: { kind: 'wait-failed', message: 'actual wait failed' } },
	])
		expect(() => requireQuotaLoginParentExit(invalid, 7)).toThrow();
	expect(() => requireQuotaLoginParentExit(valid, 0)).toThrow();
});
