/** Checks over observations from real recording seams. Schema fixtures alone prove no effect. */
import { z } from 'zod';

import { canonicalJson } from './source-binding.ts';
export const accountTargetSchema = z.strictObject({
	accountId: z.string().min(1),
	label: z.string().min(1),
	provider: z.string().min(1),
	routerRoot: z.string().min(1),
});
export type AccountTarget = z.infer<typeof accountTargetSchema>;
export const accountCommandResultSchema = z.discriminatedUnion('kind', [
	z.strictObject({ kind: z.literal('succeeded') }),
	z.strictObject({ kind: z.literal('cancelled') }),
	z.strictObject({ kind: z.literal('refused'), reason: z.string().min(1) }),
	z.strictObject({ kind: z.literal('failed'), message: z.string().min(1) }),
]);
export const accountCommandObservationSchema = z.strictObject({
	capturedTarget: accountTargetSchema,
	invocations: z.array(
		z.strictObject({
			argv: z.array(z.string()),
			routerRoot: z.string().min(1),
			termiosAtCommand: z.string().min(1),
			alternateScreen: z.boolean(),
		}),
	),
	termiosBefore: z.string().min(1),
	result: accountCommandResultSchema,
	returnedToQuota: z.boolean(),
	selectedAccountId: z.string().nullable(),
	statusAfter: z.string(),
	message: z.string(),
});
export type AccountCommandObservation = z.infer<typeof accountCommandObservationSchema>;
export function checkAccountCommand(
	expected: AccountTarget,
	argv: readonly string[],
	value: unknown,
): readonly string[] {
	const observed = accountCommandObservationSchema.parse(value);
	const failures: string[] = [];
	if (canonicalJson(observed.capturedTarget) !== canonicalJson(expected))
		failures.push('command target differs from captured Details account');
	if (observed.invocations.length !== 1) failures.push('account command must run exactly once');
	for (const invocation of observed.invocations) {
		if (canonicalJson(invocation.argv) !== canonicalJson(argv))
			failures.push('account command argv differs from owning route');
		if (invocation.routerRoot !== expected.routerRoot)
			failures.push('account command changed Router root');
		if (invocation.alternateScreen || invocation.termiosAtCommand !== observed.termiosBefore)
			failures.push('account command ran before terminal restoration');
	}
	if (!observed.returnedToQuota) failures.push('account command did not return to quota');
	return failures;
}
export function checkLoginReturn(expected: AccountTarget, value: unknown): readonly string[] {
	const observed = accountCommandObservationSchema.parse(value);
	const failures = [
		...checkAccountCommand(
			expected,
			['account', 'login', '--provider', expected.provider, '--label', expected.label],
			observed,
		),
	];
	if (observed.selectedAccountId !== expected.accountId)
		failures.push('login changed selected account');
	switch (observed.result.kind) {
		case 'succeeded':
			if (observed.statusAfter !== 'available')
				failures.push('login restarted with stale account report');
			break;
		case 'cancelled':
			if (!observed.message.includes('Login cancelled'))
				failures.push('cancelled login lacks its plain message');
			break;
		case 'failed':
		case 'refused':
			if (!observed.message.trim()) failures.push('unsuccessful login has no plain result');
			break;
	}
	return failures;
}
export function checkRemovalReturn(options: {
	readonly target: AccountTarget;
	readonly beforeIds: readonly string[];
	readonly expectedAfterIds: readonly string[];
	readonly actualAfterIds: readonly string[];
	readonly selectedAfter: string | null;
	readonly message: string;
}): readonly string[] {
	const failures: string[] = [];
	const beforeIndex = options.beforeIds.indexOf(options.target.accountId);
	if (beforeIndex < 0) failures.push('captured account absent before removal');
	if (
		canonicalJson(options.actualAfterIds) !== canonicalJson(options.expectedAfterIds) ||
		options.actualAfterIds.includes(options.target.accountId)
	)
		failures.push('removal report differs from fresh fixture expectation');
	const replacement =
		options.expectedAfterIds[Math.min(beforeIndex, options.expectedAfterIds.length - 1)] ?? null;
	if (options.selectedAfter !== replacement)
		failures.push('removal did not select the account at the former position or last survivor');
	if (!options.message.includes('Removed ' + options.target.label))
		failures.push('removal lacks named result');
	return failures;
}
const failedAccountSchema = z.strictObject({
	accountId: z.string().min(1),
	message: z.string().min(1),
});
const byAccount = (left: { accountId: string }, right: { accountId: string }): number =>
	left.accountId.localeCompare(right.accountId);
export const accountRefreshOutcomeSchema = z.strictObject({
	accountId: z.string().min(1),
	outcome: z.discriminatedUnion('kind', [
		z.strictObject({ kind: z.literal('refreshed') }),
		z.strictObject({ kind: z.literal('failed'), reason: z.string().min(1) }),
		z.strictObject({ kind: z.literal('skipped'), reason: z.string().min(1) }),
	]),
});
export const quotaRefreshResultSchema = z.discriminatedUnion('kind', [
	z.strictObject({ kind: z.literal('refreshed') }),
	z.strictObject({ kind: z.literal('partial'), failed: z.array(failedAccountSchema).min(1) }),
	z.strictObject({
		kind: z.literal('failed'),
		failed: z.array(failedAccountSchema),
		overall: z.string().min(1).nullable(),
	}),
]);
export const refreshObservationSchema = z.strictObject({
	// Independently observed service return, not inferred from the UI result being checked.
	sourceResult: z.discriminatedUnion('kind', [
		z.strictObject({ kind: z.literal('ok') }),
		z.strictObject({ kind: z.literal('all-failed') }),
		z.strictObject({ kind: z.literal('setup-failed'), message: z.string().min(1) }),
	]),
	serviceCalls: z.number().int().nonnegative(),
	callsWhileHeld: z.number().int().nonnegative(),
	events: z.array(
		z.enum([
			'started',
			'duplicate-key-acknowledged',
			'duplicate-menu-acknowledged',
			'released',
			'settled',
		]),
	),
	accountOutcomes: z.array(accountRefreshOutcomeSchema),
	result: quotaRefreshResultSchema,
	selectedBefore: z.string().min(1),
	selectedAfter: z.string().min(1),
	screenStayedOpen: z.boolean(),
	refreshingVisible: z.boolean(),
	refreshDisabledReason: z.string(),
	freshValuesVisible: z.boolean(),
	openDetailsFresh: z.boolean(),
	visibleFailureAccountIds: z.array(z.string().min(1)),
});
export function checkRefreshObservation(value: unknown): readonly string[] {
	const observed = refreshObservationSchema.parse(value);
	const failures: string[] = [];
	if (observed.serviceCalls !== 1 || observed.callsWhileHeld !== 1)
		failures.push('refresh did not retain one in-flight invocation');
	const release = observed.events.indexOf('released');
	if (
		observed.events[0] !== 'started' ||
		release < 1 ||
		observed.events.at(-1) !== 'settled' ||
		!['duplicate-key-acknowledged', 'duplicate-menu-acknowledged'].every((event) => {
			const index = observed.events.indexOf(
				event === 'duplicate-key-acknowledged'
					? 'duplicate-key-acknowledged'
					: 'duplicate-menu-acknowledged',
			);
			return index > 0 && index < release;
		})
	)
		failures.push('refresh causal duplicate acknowledgments did not precede release');
	if (!observed.refreshingVisible || !observed.refreshDisabledReason.includes('Refreshing'))
		failures.push('pending refresh lacks visible/unavailable state');
	if (!observed.screenStayedOpen || observed.selectedBefore !== observed.selectedAfter)
		failures.push('refresh left the screen or changed selection');
	const ids = observed.accountOutcomes.map((outcome) => outcome.accountId);
	if (new Set(ids).size !== ids.length) failures.push('duplicate per-account refresh outcomes');
	const failed = observed.accountOutcomes
		.filter((outcome) => outcome.outcome.kind === 'failed')
		.map((outcome) => outcome.accountId)
		.toSorted();
	const refreshed = observed.accountOutcomes.filter(
		(outcome) => outcome.outcome.kind === 'refreshed',
	);
	const result = observed.result;
	const source = observed.sourceResult;
	const expectedKind =
		source.kind === 'ok' ? (failed.length === 0 ? 'refreshed' : 'partial') : 'failed';
	if (result.kind !== expectedKind)
		failures.push('UI refresh result differs from this service return');
	if (source.kind === 'setup-failed') {
		if (
			observed.accountOutcomes.length !== 0 ||
			result.kind !== 'failed' ||
			result.failed.length !== 0 ||
			result.overall !== source.message
		)
			failures.push('setup/read error lost its original message or invented attempted accounts');
	} else if (source.kind === 'all-failed' && (failed.length === 0 || refreshed.length !== 0)) {
		failures.push('all-failed source return has inconsistent per-account outcomes');
	}
	if (result.kind === 'refreshed') {
		if (failed.length !== 0) failures.push('service partial/all failure was reported as Refreshed');
		if (observed.visibleFailureAccountIds.length !== 0)
			failures.push('all-success retained stale failure messages');
	} else {
		const expectedFailures = observed.accountOutcomes.flatMap((account) =>
			account.outcome.kind === 'failed'
				? [{ accountId: account.accountId, message: account.outcome.reason }]
				: [],
		);
		if (
			canonicalJson(result.failed.toSorted(byAccount)) !==
			canonicalJson(expectedFailures.toSorted(byAccount))
		)
			failures.push('typed refresh result changed a per-account reason');
		if (
			canonicalJson(result.failed.map((account) => account.accountId).toSorted()) !==
			canonicalJson(failed)
		)
			failures.push('typed refresh result lost or invented failing accounts');
		if (canonicalJson(observed.visibleFailureAccountIds.toSorted()) !== canonicalJson(failed))
			failures.push('visible failures differ from this invocation');
		if (
			result.kind === 'failed' &&
			failed.length === 0 &&
			(result.overall === null || observed.accountOutcomes.length !== 0)
		)
			failures.push('setup/read failure needs its overall message and no attempted account list');
		if (result.kind === 'failed' && failed.length !== 0 && result.overall !== null)
			failures.push('all-failed account error incorrectly became setup failure');
	}
	if (refreshed.length !== 0 && (!observed.freshValuesVisible || !observed.openDetailsFresh))
		failures.push('successful refresh left stale table or Details');
	return failures;
}
