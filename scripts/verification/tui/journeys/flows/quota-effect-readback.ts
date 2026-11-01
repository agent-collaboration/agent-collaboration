/** Independent checks over S7's actual final receipt, never inferred UI booleans.
 * This reader does not establish native input delivery, frame flushing or cleanup.
 */
import { z } from 'zod';

import { canonicalJson } from '../../evidence/source-binding.ts';
import { inputDispatchReceiptSchema } from '../../sessions/input-consumption-schema.ts';
import { accountRefreshOutcomeSchema, quotaRefreshResultSchema } from '../scenes/effect-checks.ts';
import {
	quotaAccountCount,
	quotaCapturedPosition,
	quotaEffectScenarioSchema,
	quotaLoginExit,
} from './quota-fixture-contract.ts';
import type { QuotaEffectScenario } from './quota-fixture-contract.ts';

const accountFactSchema = z.strictObject({
	accountId: z.string(),
	provider: z.string(),
	label: z.string(),
	status: z.enum(['enabled', 'disabled']),
	credentialGeneration: z.number().int().nullable(),
});
const targetSchema = z.strictObject({
	accountId: z.string(),
	provider: z.string(),
	label: z.string(),
	position: z.number().int(),
});
const commandSchema = targetSchema.extend({
	action: z.enum(['copy-id', 'disable', 'enable', 'remove']),
});
const commandResultSchema = z.discriminatedUnion('kind', [
	z.strictObject({ kind: z.literal('succeeded') }),
	z.strictObject({ kind: z.literal('refused'), reason: z.string() }),
	z.strictObject({ kind: z.literal('failed'), message: z.string() }),
]);
const capturedSchema = z.discriminatedUnion('kind', [
	targetSchema.extend({ kind: z.literal('details') }),
	z.strictObject({ kind: z.literal('options'), accountId: z.string(), label: z.string() }),
	z.strictObject({ kind: z.literal('floor'), accountId: z.string(), label: z.string() }),
	z.strictObject({ kind: z.literal('other') }),
]);
// Published geometry and other owner metadata remain in the original receipt.
const poseSchema = z.object({
	mount: z.number().int().positive(),
	browseAccountId: z.string().nullable(),
	listSelection: z.string().nullable(),
	captured: capturedSchema.nullable(),
	rows: z.array(z.object({ accountId: z.string(), loginState: z.string() })),
});
const serviceSourceSchema = z.strictObject({
	kind: z.enum(['ok', 'all-failed', 'service-error']),
	accountOutcomes: z.array(accountRefreshOutcomeSchema),
	message: z.string().nullable(),
});
const refreshEventSchema = z.discriminatedUnion('kind', [
	z.strictObject({ kind: z.literal('service-returned'), source: serviceSourceSchema }),
	z.strictObject({ kind: z.literal('mapping-consumed'), source: serviceSourceSchema }),
	z.strictObject({ kind: z.literal('setup-failed'), message: z.string() }),
	z.strictObject({ kind: z.literal('mapping-returned'), result: quotaRefreshResultSchema }),
]);
export const quotaRefreshReceiptSchema = z.strictObject({
	serviceCalls: z.number().int(),
	callsWhileHeld: z.number().int(),
	released: z.boolean(),
	observations: z.array(refreshEventSchema),
	requests: z.array(z.object({ method: z.string() })),
});
const loginSchema = z.strictObject({
	arguments: z.array(z.string()),
	exitCode: z.number().int(),
	terminal: z.discriminatedUnion('kind', [
		z.strictObject({ kind: z.literal('notTerminal') }),
		z.strictObject({
			kind: z.literal('interactive'),
			canonical: z.boolean(),
			echo: z.boolean(),
			signals: z.boolean(),
			foreground: z.boolean(),
		}),
	]),
});
const loginParentExitSchema = z.strictObject({
	childPid: z.number().int().positive().nullable(),
	waitOrdinal: z.number().int(),
	status: z.discriminatedUnion('kind', [
		z.strictObject({
			kind: z.literal('exited'),
			code: z.number().int(),
			rawStatus: z.number().int(),
		}),
		z.strictObject({
			kind: z.literal('signaled'),
			signal: z.number().int(),
			rawStatus: z.number().int(),
		}),
		z.strictObject({ kind: z.literal('wait-failed'), message: z.string() }),
	]),
});
/** Source-qualified parent wait, independent of the stand-in's planned exit code. */
export function requireQuotaLoginParentExit(
	value: unknown,
	expected: 0 | 130 | 7,
): { readonly kind: 'exited'; readonly code: number } {
	const parent = loginParentExitSchema.parse(value);
	if (
		parent.childPid === null ||
		parent.waitOrdinal !== 1 ||
		parent.status.kind !== 'exited' ||
		parent.status.code !== expected ||
		parent.status.rawStatus !== expected << 8
	)
		throw new Error(
			'actual parent login wait identity, count or raw status differs from declared outcome',
		);
	return { kind: 'exited', code: parent.status.code };
}
const policyFactsSchema = z.strictObject({
	floors: z.array(z.unknown()),
	credits: z.array(z.strictObject({ accountId: z.string(), policy: z.string() })),
});
export const quotaEffectReceiptSchema = z.strictObject({
	scenario: quotaEffectScenarioSchema,
	outcomes: z.array(
		z.discriminatedUnion('kind', [
			z.strictObject({ kind: z.literal('quit') }),
			z.strictObject({
				kind: z.literal('login-again'),
				accountId: z.string(),
				provider: z.string(),
				label: z.string(),
			}),
		]),
	),
	before: z.array(accountFactSchema),
	after: z.array(accountFactSchema),
	policiesBefore: policyFactsSchema,
	policiesAfter: policyFactsSchema,
	accountCommands: z.array(z.strictObject({ command: commandSchema, result: commandResultSchema })),
	clipboard: z.array(z.string()),
	removalGateEntered: z.boolean(),
	removalGateReleased: z.boolean(),
	refresh: quotaRefreshReceiptSchema.nullable(),
	loginChild: loginSchema.nullable(),
	loginParentExit: loginParentExitSchema.nullable(),
	resetRequests: z.array(z.unknown()),
	fixtureMutations: z.array(
		z.object({ kind: z.literal('fixture-mutation'), operation: z.string() }),
	),
	unexpectedEffectCalls: z.array(z.string()),
	publishedPoses: z.array(poseSchema).min(1),
	nativeDispatch: z.array(inputDispatchReceiptSchema).min(1),
});
export type QuotaEffectReceipt = z.infer<typeof quotaEffectReceiptSchema>;

export function checkQuotaRefreshReceipt(
	scenario: QuotaEffectScenario,
	value: unknown,
): readonly string[] {
	if (!scenario.startsWith('refresh-'))
		throw new Error('refresh reader requires a refresh scenario');
	const receipt = quotaRefreshReceiptSchema.parse(value);
	const failures: string[] = [];
	if (receipt.serviceCalls !== 1 || receipt.callsWhileHeld !== 1 || !receipt.released)
		failures.push('refresh must settle one causally held invocation');
	const sources = receipt.observations.filter(
		(event) => event.kind === 'service-returned' || event.kind === 'setup-failed',
	);
	const consumed = receipt.observations.filter((event) => event.kind === 'mapping-consumed');
	const mapped = receipt.observations.filter((event) => event.kind === 'mapping-returned');
	const source = sources[0],
		mapping = mapped[0];
	if (
		sources.length !== 1 ||
		mapped.length !== 1 ||
		source === undefined ||
		mapping?.kind !== 'mapping-returned'
	)
		return [...failures, 'refresh needs exactly one source return and one mapper return'];
	const expectedKind =
		scenario === 'refresh-all-success'
			? 'refreshed'
			: scenario === 'refresh-partial'
				? 'partial'
				: 'failed';
	if (mapping.result.kind !== expectedKind)
		failures.push('mapper kind differs from declared outcome');
	if (source.kind === 'setup-failed') {
		if (
			scenario !== 'refresh-setup-failed' ||
			consumed.length !== 0 ||
			mapping.result.kind !== 'failed' ||
			mapping.result.failed.length !== 0 ||
			mapping.result.overall !== source.message
		)
			failures.push('setup error lost its original message or invented attempted accounts');
	} else {
		const expectedSource = scenario === 'refresh-all-failed' ? 'all-failed' : 'ok';
		if (
			scenario === 'refresh-setup-failed' ||
			source.source.kind !== expectedSource ||
			consumed.length !== 1 ||
			canonicalJson(consumed[0]?.source) !== canonicalJson(source.source)
		)
			failures.push('service return and mapper input are not the same actual invocation');
		const outcomes = source.source.accountOutcomes;
		const expected = ['effect-account-0', 'effect-account-1'].map((accountId, index) => ({
			accountId,
			kind:
				scenario === 'refresh-all-failed' || (scenario === 'refresh-partial' && index === 1)
					? 'failed'
					: 'refreshed',
		}));
		if (
			canonicalJson(
				outcomes
					.map((entry) => ({ accountId: entry.accountId, kind: entry.outcome.kind }))
					.toSorted((left, right) => left.accountId.localeCompare(right.accountId)),
			) !== canonicalJson(expected)
		)
			failures.push('refresh account outcomes differ from the independently seeded accounts');
		const failed = outcomes.flatMap((entry) =>
			entry.outcome.kind === 'failed'
				? [{ accountId: entry.accountId, message: entry.outcome.reason }]
				: [],
		);
		if (
			mapping.result.kind !== 'refreshed' &&
			(canonicalJson(
				mapping.result.failed.toSorted((left, right) =>
					left.accountId.localeCompare(right.accountId),
				),
			) !==
				canonicalJson(
					failed.toSorted((left, right) => left.accountId.localeCompare(right.accountId)),
				) ||
				(mapping.result.kind === 'failed' && mapping.result.overall !== null))
		)
			failures.push('mapper lost or changed per-account reasons');
	}
	const requests =
		scenario === 'refresh-all-success'
			? 8
			: scenario === 'refresh-partial'
				? 6
				: scenario === 'refresh-all-failed'
					? 4
					: 0;
	if (
		receipt.requests.length !== requests ||
		receipt.requests.some((request) => request.method !== 'GET')
	)
		failures.push('refresh request set contains a retry or unexpected provider effect');
	return failures;
}

export function checkQuotaEffectReceipt(options: {
	readonly scenario: QuotaEffectScenario;
	readonly routerRoot: string;
	readonly value: unknown;
}): readonly string[] {
	const receipt = quotaEffectReceiptSchema.parse(options.value);
	const failures: string[] = [];
	const scenario = options.scenario,
		position = quotaCapturedPosition(scenario),
		loginExit = quotaLoginExit(scenario);
	const target = {
		accountId: `effect-account-${position}`,
		provider: 'openai',
		label: `effect-${['alpha', 'beta', 'gamma'][position]}`,
		position,
	};
	if (receipt.scenario !== scenario) failures.push('receipt scenario differs from declaration');
	const before = Array.from({ length: quotaAccountCount(scenario) }, (_, index) => ({
		accountId: `effect-account-${index}`,
		provider: loginExit !== null && index === 1 ? 'claude' : 'openai',
		label: `effect-${['alpha', 'beta', 'gamma'][index]}`,
		status: scenario.startsWith('remove-') && index === position ? 'disabled' : 'enabled',
		credentialGeneration: 7,
	}));
	if (canonicalJson(receipt.before) !== canonicalJson(before))
		failures.push('initial account facts differ from declared seed');
	const removed = [
		'captured-actions',
		'remove-middle',
		'remove-last',
		'remove-only',
		'remove-held',
	].includes(scenario);
	const after = before
		.filter((account) => !removed || account.accountId !== target.accountId)
		.map((account) =>
			Object.assign({}, account, {
				status:
					account.accountId === target.accountId && scenario === 'remove-requires-disabled'
						? 'enabled'
						: account.status,
				credentialGeneration:
					account.accountId === target.accountId && scenario === 'login-succeeded'
						? 8
						: account.credentialGeneration,
			}),
		);
	if (canonicalJson(receipt.after) !== canonicalJson(after))
		failures.push('persisted account effects differ from exact scenario');
	const actions =
		scenario === 'captured-actions'
			? ['copy-id', 'disable', 'enable']
			: scenario.startsWith('remove-') && scenario !== 'remove-cancel'
				? ['remove']
				: [];
	if (
		canonicalJson(receipt.accountCommands.map((entry) => entry.command)) !==
		canonicalJson(actions.map((action) => ({ ...target, action })))
	)
		failures.push('command set retargeted captured A, repeated or invoked an unrelated action');
	for (const command of receipt.accountCommands)
		if (
			canonicalJson(command.result) !==
			canonicalJson(
				scenario === 'remove-requires-disabled'
					? { kind: 'refused', reason: 'RequiresDisabled' }
					: { kind: 'succeeded' },
			)
		)
			failures.push('typed account result differs from scenario');
	if (
		canonicalJson(receipt.clipboard) !==
		canonicalJson(scenario === 'captured-actions' ? [target.accountId] : [])
	)
		failures.push('clipboard lost the full captured account ID or ran unexpectedly');
	const mutation =
		scenario === 'captured-actions'
			? ['remove-captured-account-externally']
			: scenario === 'remove-requires-disabled'
				? ['reenable-before-removal']
				: scenario === 'login-succeeded'
					? ['activate-fresh-login-generation']
					: [];
	if (
		canonicalJson(receipt.fixtureMutations.map((entry) => entry.operation)) !==
		canonicalJson(mutation)
	)
		failures.push('setup mutation ledger differs from declared scenario');
	if (
		receipt.unexpectedEffectCalls.length !== 0 ||
		receipt.resetRequests.length !== 0 ||
		receipt.policiesBefore.floors.length !== 0 ||
		receipt.policiesAfter.floors.length !== 0 ||
		canonicalJson(receipt.policiesBefore.credits.map((entry) => entry.accountId).toSorted()) !==
			canonicalJson(before.map((entry) => entry.accountId).toSorted()) ||
		canonicalJson(receipt.policiesAfter.credits) !==
			canonicalJson(
				receipt.policiesBefore.credits.filter((entry) =>
					after.some((account) => account.accountId === entry.accountId),
				),
			)
	)
		failures.push('unrelated policy, reset or effect-port mutation');
	const expectedOutcomes =
		loginExit === null
			? [{ kind: 'quit' }]
			: [
					{
						kind: 'login-again',
						accountId: target.accountId,
						provider: target.provider,
						label: target.label,
					},
					{ kind: 'quit' },
				];
	if (canonicalJson(receipt.outcomes) !== canonicalJson(expectedOutcomes))
		failures.push('fixture did not produce exact captured login/guarded Quit outcomes');
	if (loginExit === null ? receipt.loginChild !== null : receipt.loginChild === null)
		failures.push('login child presence differs from scenario');
	if (loginExit === null && receipt.loginParentExit !== null)
		failures.push('non-login scenario observed a parent login wait');
	if (loginExit !== null) {
		try {
			requireQuotaLoginParentExit(receipt.loginParentExit, loginExit);
		} catch {
			failures.push(
				'actual parent login wait identity, count or raw status differs from declared outcome',
			);
		}
	}
	if (loginExit !== null && receipt.loginChild !== null) {
		const child = receipt.loginChild;
		if (
			child.exitCode !== loginExit ||
			canonicalJson(child.arguments) !==
				canonicalJson([
					'account',
					'login',
					'--provider',
					target.provider,
					'--label',
					target.label,
					'--router-root',
					options.routerRoot,
				])
		)
			failures.push('login child argv or planned exit differs from captured target');
		if (
			child.terminal.kind !== 'interactive' ||
			!child.terminal.canonical ||
			!child.terminal.echo ||
			!child.terminal.signals ||
			!child.terminal.foreground
		)
			failures.push('login child entered before terminal restoration');
		if (new Set(receipt.publishedPoses.map((pose) => pose.mount)).size < 2)
			failures.push('actual CLI restart mount absent');
		if (
			scenario === 'login-succeeded' &&
			!receipt.publishedPoses
				.at(-1)
				?.rows.some((row) => row.accountId === target.accountId && row.loginState === 'NotRequired')
		)
			failures.push('login returned with stale account report');
	}
	if (scenario === 'captured-actions' || loginExit !== null)
		for (const kind of scenario === 'captured-actions'
			? ['details', 'options', 'floor']
			: ['details'])
			if (
				!receipt.publishedPoses.some(
					(pose) =>
						pose.captured?.kind === kind &&
						'accountId' in pose.captured &&
						pose.captured.accountId === target.accountId &&
						pose.browseAccountId === 'effect-account-1',
				)
			)
				failures.push(`captured ${kind} A / Browse B pose absent`);
	if (scenario === 'remove-held' && (!receipt.removalGateEntered || !receipt.removalGateReleased))
		failures.push('real removal commit gate did not enter and release');
	if (removed && scenario !== 'captured-actions') {
		const replacement = after[Math.min(position, after.length - 1)]?.accountId ?? null;
		if (receipt.publishedPoses.at(-1)?.listSelection !== replacement)
			failures.push('removal selection differs from old position or last survivor');
	}
	const nativeActions = receipt.nativeDispatch
		.map((dispatch) => dispatch.action)
		.filter((action) => action !== null);
	if (
		nativeActions.some((action) =>
			[
				'quota:save-credit',
				'quota:refresh-credit',
				'quota:floor-save',
				'quota:reset-advance',
				'quota:inspect-reset',
			].includes(action),
		)
	)
		failures.push('accepted an unrelated policy or reset owner action');
	for (const [action, expectedCount] of [
		['quota:copy-id', actions.filter((value) => value === 'copy-id').length],
		[
			'quota:account-status',
			actions.filter((value) => value === 'enable' || value === 'disable').length,
		],
		['quota:confirm-remove', actions.filter((value) => value === 'remove').length],
		['quota:login', loginExit === null ? 0 : 1],
		['quota:refresh', scenario.startsWith('refresh-') ? 1 : 0],
	] as const)
		if (nativeActions.filter((value) => value === action).length !== expectedCount)
			failures.push(`native acceptance count differs for ${action}`);
	if (scenario.startsWith('refresh-'))
		failures.push(...checkQuotaRefreshReceipt(scenario, receipt.refresh));
	else if (receipt.refresh !== null) failures.push('non-refresh scenario called refresh');
	return failures;
}
