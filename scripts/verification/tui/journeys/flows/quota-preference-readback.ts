/** Actual saved choices and reopened store facts; a planned facet never proves persistence. */
import { join } from 'node:path';

import { z } from 'zod';

import { canonicalJson } from '../../evidence/source-binding.ts';
import type { QuotaEffectScenario } from './quota-fixture-contract.ts';

const facetSchema = z.enum(['none', 'provider', 'status']);
const invocationSchema = z.strictObject({
	invocation: z.number().int().positive(),
	initialMount: z.number().int().positive(),
	finalMount: z.number().int().positive(),
	initialFacet: facetSchema,
	finalFacet: facetSchema,
	storedValue: z.string().nullable(),
});
export const quotaPreferenceReceiptSchema = z.strictObject({
	groupingReopen: z
		.strictObject({
			storePath: z.string().startsWith('/'),
			invocations: z.tuple([invocationSchema, invocationSchema]),
		})
		.nullable(),
	preferenceWrites: z.array(
		z.strictObject({
			ordinal: z.number().int().positive(),
			facet: facetSchema,
			result: z.discriminatedUnion('kind', [
				z.strictObject({ kind: z.literal('saved') }),
				z.strictObject({ kind: z.literal('failed') }),
			]),
		}),
	),
	storedGrouping: z.string().nullable(),
});

export function checkQuotaPreferences(options: {
	readonly scenario: QuotaEffectScenario;
	readonly routerRoot: string;
	readonly facts: z.infer<typeof quotaPreferenceReceiptSchema>;
	readonly nativeActions: readonly string[];
	readonly poses: readonly { readonly mount: number; readonly checkedGroups: readonly string[] }[];
}): readonly string[] {
	const failures: string[] = [],
		{ facts, nativeActions, scenario, poses } = options;
	const cycles = nativeActions.filter((action) => action === 'quota:group-cycle').length;
	const expected =
		scenario === 'grouping-reopen' && cycles === 2
			? ['provider', 'status']
			: scenario === 'login-succeeded' && cycles === 1
				? ['provider']
				: [];
	if (
		cycles !== expected.length ||
		nativeActions.some(
			(action) => action.startsWith('quota:group-') && action !== 'quota:group-cycle',
		)
	)
		failures.push('undeclared native grouping preference route');
	if (
		canonicalJson(facts.preferenceWrites) !==
		canonicalJson(
			expected.map((facet, index) => ({ ordinal: index + 1, facet, result: { kind: 'saved' } })),
		)
	)
		failures.push('actual preference save count, order or result differs from native choices');
	if (facts.storedGrouping !== (expected.at(-1) ?? null))
		failures.push('actual final grouping store differs from the last accepted saved choice');
	const checked = (mount: number, facet: string): boolean =>
		poses.some(
			(pose) =>
				pose.mount === mount &&
				canonicalJson(pose.checkedGroups) === canonicalJson(['quota:group-' + facet]),
		);
	if (scenario !== 'grouping-reopen') {
		if (facts.groupingReopen !== null)
			failures.push('non-grouping scenario invented invocation reopen facts');
		if (
			scenario === 'login-succeeded' &&
			cycles === 1 &&
			(!checked(1, 'provider') || !checked(2, 'provider'))
		)
			failures.push(
				'saved Provider choice was not published before and after the real login remount',
			);
		return failures;
	}
	const grouping = facts.groupingReopen;
	if (grouping === null) return [...failures, 'actual grouping invocation facts absent'];
	const [first, second] = grouping.invocations,
		mount = first.initialMount;
	if (
		grouping.storePath !== join(options.routerRoot, 'agent-proxy-ui.sqlite') ||
		canonicalJson(grouping.invocations) !==
			canonicalJson([
				{
					invocation: 1,
					initialMount: mount,
					finalMount: mount,
					initialFacet: 'none',
					finalFacet: 'provider',
					storedValue: 'provider',
				},
				{
					invocation: 2,
					initialMount: mount + 1,
					finalMount: mount + 1,
					initialFacet: 'provider',
					finalFacet: 'status',
					storedValue: 'status',
				},
			])
	)
		failures.push(
			'actual grouping store, invocation, facet or mount differs from the declared reopen',
		);
	if (
		canonicalJson(
			nativeActions.filter((action) => action === 'quota:group-cycle' || action === 'quota:quit'),
		) !== canonicalJson(['quota:group-cycle', 'quota:quit', 'quota:group-cycle', 'quota:quit'])
	)
		failures.push('grouping needs two ordered real activations and guarded Quits');
	if (
		!checked(mount, 'none') ||
		!checked(mount, 'provider') ||
		!checked(second.initialMount, 'provider') ||
		!checked(second.initialMount, 'status') ||
		poses.some((pose) => pose.mount !== mount && pose.mount !== mount + 1)
	)
		failures.push('actual checked grouping poses do not cover both mounted invocations');
	return failures;
}
