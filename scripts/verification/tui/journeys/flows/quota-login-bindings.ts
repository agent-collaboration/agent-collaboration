/** Native login journeys over S7's real CLI fixture, preserved PTY, and independent parent wait. */
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { z } from 'zod';

import { requireHandoffChildEntry } from '../../terminal/loop-ending-handoff.ts';
import type { MarkedInputEvidence } from '../../terminal/marker-input-session.ts';
import { gridLines } from '../../terminal/terminal-grid.ts';
import type { AuthoredFlowPlan } from './authored-flow-plan.ts';
import type { FlowCheck } from './flow-definition.ts';
import type { FlowInvocation } from './flow-executor.ts';
import type {
	NativeFlowBindings,
	NativeFlowContext,
	NativeFlowInputBinding,
} from './native-flow-runtime.ts';
import { requireQuotaLoginParentExit } from './quota-effect-readback.ts';
import { quotaJourneyScenarios, quotaLoginExit } from './quota-fixture-contract.ts';
import type { QuotaFixtureLayout } from './quota-fixture-layout.ts';
import {
	collectQuotaDialog,
	quotaBaseBindings,
	quotaDialogTarget,
	quotaExpectedKey,
	quotaMouse,
	quotaOverlay,
	quotaPrimary,
	quotaScreen,
} from './quota-flow-inputs.ts';
import { quotaLoginHandoff } from './quota-handoff-bindings.ts';
import { quotaObservationSchema } from './quota-live-fixture.ts';
import type { QuotaLiveFixture } from './quota-live-fixture.ts';

const observationSchema = quotaObservationSchema;
const loginEntrySchema = z.strictObject({
	kind: z.literal('login-child-entered'),
	receipt: z.unknown(),
});
const parentEventSchema = z
	.object({ kind: z.literal('login-parent-exited') })
	.catchall(z.unknown());

export function quotaLoginBindings(options: {
	readonly plan: AuthoredFlowPlan;
	readonly invocation: FlowInvocation;
	readonly evidence: string;
	readonly layout: QuotaFixtureLayout;
	readonly fixture: QuotaLiveFixture;
}): NativeFlowBindings | null {
	const { plan, invocation, evidence, layout, fixture } = options;
	const scenario = quotaJourneyScenarios(invocation.journey.journeyId)[0];
	if (scenario === undefined) return null;
	const exit = quotaLoginExit(scenario);
	if (exit === null) return null;
	const base = quotaBaseBindings({ plan, invocation, evidence, layout });
	const inputs = new Map<number, NativeFlowInputBinding>();
	let enteredDetails = 0;
	for (const [index, step] of invocation.journey.steps.entries()) {
		if (step.kind === 'press' && step.key === 'Enter') {
			enteredDetails += 1;
			inputs.set(index, {
				kind: 'continuing',
				expected: [
					quotaExpectedKey('Enter', quotaScreen, 'action:quota:accounts', 'quota:details'),
				],
			});
		} else if (step.kind === 'click' && step.target === 'Log in again') {
			inputs.set(index, {
				kind: 'click',
				ending: 'handoff',
				expected: (location) => [
					quotaMouse(location, 'down', quotaOverlay(1), 'action:quota:login', 'quota:login'),
				],
				handoff: async (context) => {
					const access = await fixture.access();
					const live = await fixture.read(context.frame.binding.sequence);
					if (
						live.readback.captured?.kind !== 'details' ||
						live.readback.captured.accountId !== 'effect-account-0' ||
						live.readback.browseAccountId !== 'effect-account-1'
					)
						throw new Error('login lost captured A / Browse B before activation');
					const argv = [
						'account',
						'login',
						'--provider',
						'openai',
						'--label',
						'effect-alpha',
						'--router-root',
						access.started.routerRoot,
					];
					return quotaLoginHandoff({
						...access,
						afterSequence: access.journal.read().length,
						previousMount: live.readback.mount,
						expectedChildArguments: argv,
						expectedChildExitCode: exit,
						ready: base.ready,
						firstFrame: base.initialBarrier,
					});
				},
			});
		}
	}
	if (enteredDetails !== (exit === 0 ? 2 : 1))
		throw new Error('login authored details activation count changed');
	let detailsBody: readonly string[] = [];
	const read = async (context: NativeFlowContext): Promise<z.infer<typeof observationSchema>> => {
		fixture.invalidate();
		const { journal, started } = await fixture.access();
		return {
			live: await fixture.read(context.frame.binding.sequence),
			events: [...journal.read()],
			routerRoot: started.routerRoot,
		};
	};
	const inspect = async (context: NativeFlowContext): Promise<MarkedInputEvidence> => {
		const result = await collectQuotaDialog({
			context,
			title: 'Account details',
			surface: quotaOverlay(1),
			focusedTarget: 'action:quota:account-actions',
		});
		detailsBody = result.rows;
		writeFileSync(
			join(evidence, 'login-details-full-body.json'),
			JSON.stringify(result.rows, null, 2),
		);
		return result.marked;
	};
	const check = async (
		name: FlowCheck,
		context: NativeFlowContext,
		value: unknown,
	): Promise<readonly string[]> => {
		const observed = value === undefined ? null : observationSchema.parse(value);
		const live = observed?.live ?? (await fixture.read(context.frame.binding.sequence));
		const pose = live.readback,
			failures: string[] = [];
		const account = pose.rows.find((row) => row.accountId === 'effect-account-0');
		if (name === 'login-primary-by-status') {
			const label = account?.loginState === 'Required' ? 'Log in again' : 'Close';
			if (!quotaPrimary(context.frame.grid, 'Account details', label))
				failures.push('Details primary mark does not match the independently observed login state');
		} else if (name === 'exact-login-command' && observed === null) {
			if (!quotaLoginCommandMatches(detailsBody))
				failures.push('full actual Details body does not contain the exact captured command');
		} else if (name === 'captured-account-target') {
			if (
				pose.captured !== null &&
				('accountId' in pose.captured ? pose.captured.accountId !== 'effect-account-0' : true)
			)
				failures.push('Details target changed from captured A');
			if (pose.captured === null && pose.listSelection !== 'effect-account-0')
				failures.push('login return selected a different account');
		} else if (name === 'command-result-and-fresh-return') {
			const text = gridLines(context.frame.grid).join(' ').replace(/\s+/gu, ' ');
			if (pose.mount !== 2 || pose.listSelection !== 'effect-account-0')
				failures.push('login did not return on a fresh mount with captured A selected');
			if (exit === 0 && account?.loginState !== 'NotRequired')
				failures.push('successful login returned stale login state');
			if (exit === 130 && !text.includes('Login cancelled: effect-alpha'))
				failures.push('cancelled login lacks its plain message');
			if (exit === 7 && !text.includes('Login failed for effect-alpha:'))
				failures.push('failed login lacks its plain message');
		} else if (observed !== null) {
			const entries = observed.events.filter(
				(record) => record.event.kind === 'login-child-entered',
			);
			const parents = observed.events.filter(
				(record) => record.event.kind === 'login-parent-exited',
			);
			if (entries.length !== 1 || parents.length !== 1)
				throw new Error('login must have one actual child entry and parent wait');
			const entry = loginEntrySchema.parse(entries[0]?.event);
			requireHandoffChildEntry(entry.receipt, [
				'account',
				'login',
				'--provider',
				'openai',
				'--label',
				'effect-alpha',
				'--router-root',
				observed.routerRoot,
			]);
			const { kind: _kind, ...parent } = parentEventSchema.parse(parents[0]?.event);
			requireQuotaLoginParentExit(parent, exit);
		} else throw new Error('quota login check has no real readback: ' + name);
		return failures;
	};
	const names: readonly FlowCheck[] = [
		'login-primary-by-status',
		'exact-login-command',
		'command-after-terminal-restore',
		'command-result-and-fresh-return',
		'captured-account-target',
		'no-retry',
	];
	return {
		...base,
		inputs,
		targets: new Map([
			['Log in again', (grid) => quotaDialogTarget(grid, 'Account details', 'Log in again')],
		]),
		fixtures: new Map<string, (context: NativeFlowContext) => Promise<MarkedInputEvidence | null>>([
			[
				'select-relogin-required-account-a',
				async (context) => {
					const live = await fixture.read(context.frame.binding.sequence);
					await fixture.setup({
						operation: 'select-browse-account',
						mount: live.readback.mount,
						accountId: 'effect-account-0',
					});
					return context.input.press('F12', [quotaExpectedKey('F12', quotaScreen)]);
				},
			],
			['inspect-account-login-details', inspect],
			[
				'move-browse-to-b-and-reload',
				async (context) => {
					const live = await fixture.read(context.frame.binding.sequence);
					await fixture.setup({
						operation: 'select-browse-account',
						mount: live.readback.mount,
						accountId: 'effect-account-1',
					});
					await fixture.setup({ operation: 'reload-view-model', mount: live.readback.mount });
					return context.input.press('F12', [quotaExpectedKey('F12', quotaOverlay(1))]);
				},
			],
			[
				'login-command-' + (exit === 0 ? 'succeeded' : exit === 130 ? 'cancelled-130' : 'failed'),
				async () => {
					if (layout.configuration.scenario !== scenario)
						throw new Error('login planned result differs from fixture');
					return null;
				},
			],
		]),
		readbacks: new Map([['account-command', read]]),
		checks: new Map(
			names.map((name) => [name, async (context, value) => check(name, context, value)]),
		),
	};
}

/** Captured command text is distinct from the later actual child argv proof. */
export function quotaLoginCommandMatches(rows: readonly string[]): boolean {
	const text = rows.join('').replace(/\s/gu, ''),
		anchor = 'codex-routeraccountlogin';
	const first = text.indexOf(anchor),
		end = text.indexOf('Quotawindows', first);
	if (first < 0 || end < first || text.indexOf(anchor, first + anchor.length) >= 0) return false;
	return text.slice(first, end) === 'codex-routeraccountlogin--provideropenai--labeleffect-alpha';
}
