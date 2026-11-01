/** Literal producer-wire controls, not a native quota or persistence observation. */
import { expect, test } from 'vitest';

import { checkQuotaEffectReceipt, quotaEffectReceiptSchema } from './quota-effect-readback.ts';

function groupingReceipt(): unknown {
	const accounts = ['openai', 'claude'].map((provider, index) => ({
		accountId: `effect-account-${index}`,
		provider,
		label: `effect-${index === 0 ? 'alpha' : 'beta'}`,
		status: 'enabled',
		credentialGeneration: 7,
	}));
	const policy = {
		floors: [],
		credits: accounts.map((account) => ({
			accountId: account.accountId,
			policy: 'fixture-default',
		})),
	};
	return {
		scenario: 'grouping-reopen',
		outcomes: [{ kind: 'quit' }, { kind: 'quit' }],
		before: accounts,
		after: accounts,
		policiesBefore: policy,
		policiesAfter: policy,
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
		groupingReopen: {
			storePath: '/fixture/agent-proxy-ui.sqlite',
			invocations: [
				{
					invocation: 1,
					initialMount: 1,
					finalMount: 1,
					initialFacet: 'none',
					finalFacet: 'provider',
					storedValue: 'provider',
				},
				{
					invocation: 2,
					initialMount: 2,
					finalMount: 2,
					initialFacet: 'provider',
					finalFacet: 'status',
					storedValue: 'status',
				},
			],
		},
		preferenceWrites: [
			{ ordinal: 1, facet: 'provider', result: { kind: 'saved' } },
			{ ordinal: 2, facet: 'status', result: { kind: 'saved' } },
		],
		storedGrouping: 'status',
		publishedPoses: [
			{ mount: 1, facet: 'none' },
			{ mount: 1, facet: 'provider' },
			{ mount: 2, facet: 'provider' },
			{ mount: 2, facet: 'status' },
		].map(({ mount, facet }) => ({
			mount,
			browseAccountId: 'effect-account-0',
			listSelection: 'effect-account-0',
			captured: null,
			rows: [],
			actions: ['none', 'provider', 'status'].map((candidate) => ({
				['id']: 'quota:group-' + candidate,
				checked: candidate === facet,
			})),
		})),
		nativeDispatch: ['quota:group-cycle', 'quota:quit', 'quota:group-cycle', 'quota:quit'].map(
			(action, index) => ({
				sequence: index + 1,
				event:
					action === 'quota:group-cycle'
						? { kind: 'key', code: { kind: 'fixed', name: 'F6' }, modifiers: [], phase: 'press' }
						: {
								kind: 'key',
								code: { kind: 'shortcut', key: 'c' },
								modifiers: ['control'],
								phase: 'press',
							},
				surface: { kind: 'screen' },
				target: null,
				action,
			}),
		),
	};
}

test('actual grouping summary requires two real invocations, Quits and ordered saved preferences', () => {
	expect(
		checkQuotaEffectReceipt({
			scenario: 'grouping-reopen',
			routerRoot: '/fixture',
			value: groupingReceipt(),
		}),
	).toEqual([]);
});

test('grouping rejects a changed store, stale mount, lost initial facet, missing write and actual failed save', () => {
	const wrongStore = quotaEffectReceiptSchema.parse(groupingReceipt());
	if (wrongStore.groupingReopen === null) throw new Error('grouping fixture absent');
	wrongStore.groupingReopen.storePath = '/other/agent-proxy-ui.sqlite';
	expect(
		checkQuotaEffectReceipt({
			scenario: 'grouping-reopen',
			routerRoot: '/fixture',
			value: wrongStore,
		}),
	).not.toEqual([]);
	const staleMount = quotaEffectReceiptSchema.parse(groupingReceipt());
	if (staleMount.groupingReopen === null) throw new Error('grouping fixture absent');
	staleMount.groupingReopen.invocations[1].initialMount = 1;
	expect(
		checkQuotaEffectReceipt({
			scenario: 'grouping-reopen',
			routerRoot: '/fixture',
			value: staleMount,
		}),
	).not.toEqual([]);
	const wrongFacet = quotaEffectReceiptSchema.parse(groupingReceipt());
	if (wrongFacet.groupingReopen === null) throw new Error('grouping fixture absent');
	wrongFacet.groupingReopen.invocations[1].initialFacet = 'none';
	expect(
		checkQuotaEffectReceipt({
			scenario: 'grouping-reopen',
			routerRoot: '/fixture',
			value: wrongFacet,
		}),
	).not.toEqual([]);
	const dropped = quotaEffectReceiptSchema.parse(groupingReceipt());
	dropped.preferenceWrites.pop();
	expect(
		checkQuotaEffectReceipt({
			scenario: 'grouping-reopen',
			routerRoot: '/fixture',
			value: dropped,
		}),
	).not.toEqual([]);
	const failed = quotaEffectReceiptSchema.parse(groupingReceipt());
	const write = failed.preferenceWrites[1];
	if (write === undefined) throw new Error('write absent');
	write.result = { kind: 'failed' };
	expect(
		checkQuotaEffectReceipt({ scenario: 'grouping-reopen', routerRoot: '/fixture', value: failed }),
	).not.toEqual([]);
});

test('grouping rejects missing native Quit or checked pose, duplicate save and invented child wait', () => {
	const quit = quotaEffectReceiptSchema.parse(groupingReceipt());
	quit.nativeDispatch.pop();
	expect(
		checkQuotaEffectReceipt({ scenario: 'grouping-reopen', routerRoot: '/fixture', value: quit }),
	).not.toEqual([]);
	const pose = quotaEffectReceiptSchema.parse(groupingReceipt());
	pose.publishedPoses = pose.publishedPoses.filter((entry) => entry.mount === 1);
	expect(
		checkQuotaEffectReceipt({ scenario: 'grouping-reopen', routerRoot: '/fixture', value: pose }),
	).not.toEqual([]);
	const doubled = quotaEffectReceiptSchema.parse(groupingReceipt());
	const write = doubled.preferenceWrites[0];
	if (write === undefined) throw new Error('write absent');
	doubled.preferenceWrites.push(write);
	expect(
		checkQuotaEffectReceipt({
			scenario: 'grouping-reopen',
			routerRoot: '/fixture',
			value: doubled,
		}),
	).not.toEqual([]);
	const child = quotaEffectReceiptSchema.parse(groupingReceipt());
	child.loginParentExit = {
		childPid: 42,
		waitOrdinal: 1,
		status: { kind: 'exited', code: 0, rawStatus: 0 },
	};
	expect(
		checkQuotaEffectReceipt({ scenario: 'grouping-reopen', routerRoot: '/fixture', value: child }),
	).not.toEqual([]);
});

test('grouping wire is strict about missing or extra invocation and summary fields', () => {
	const receipt = quotaEffectReceiptSchema.parse(groupingReceipt());
	if (receipt.groupingReopen === null) throw new Error('grouping fixture absent');
	expect(
		quotaEffectReceiptSchema.safeParse({
			...receipt,
			groupingReopen: {
				...receipt.groupingReopen,
				invocations: receipt.groupingReopen.invocations.slice(0, 1),
			},
		}).success,
	).toBe(false);
	expect(quotaEffectReceiptSchema.safeParse({ ...receipt, madeUpRemount: true }).success).toBe(
		false,
	);
	const { preferenceWrites: _removed, ...missing } = receipt;
	expect(quotaEffectReceiptSchema.safeParse(missing).success).toBe(false);
});
