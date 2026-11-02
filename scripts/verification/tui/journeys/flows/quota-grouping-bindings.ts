/** Native grouping projection plus real first-Quit settlement and second-invocation persistence. */
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';

import type { InputConsumptionExpectation } from '../../sessions/input-consumption-schema.ts';
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
import { quotaEffectReceiptSchema, checkQuotaEffectReceipt } from './quota-effect-readback.ts';
import type { QuotaFixtureLayout } from './quota-fixture-layout.ts';
import {
	quotaBaseBindings,
	quotaExpectedKey,
	quotaMarkedFrame,
	quotaOverlay,
	quotaScreen,
} from './quota-flow-inputs.ts';
import { quotaGroupingHandoff } from './quota-handoff-bindings.ts';
import type { QuotaLiveFixture, QuotaLiveReadback } from './quota-live-fixture.ts';
import { collectQuotaMenu } from './quota-menu-observation.ts';
import type { QuotaMenuObservation } from './quota-menu-observation.ts';

function statusHeading(status: string): string {
	for (const heading of ['Preferred', 'Available', 'Held', 'Blocked', 'Stale'])
		if (status === 'Account(' + heading + ')') return heading;
	if (status.startsWith('CredentialUnavailable')) return 'Credentials unavailable';
	throw new Error('grouping seed contains an unbound domain status: ' + status);
}
export function quotaGroupingBindings(options: {
	readonly plan: AuthoredFlowPlan;
	readonly invocation: FlowInvocation;
	readonly evidence: string;
	readonly layout: QuotaFixtureLayout;
	readonly fixture: QuotaLiveFixture;
}): NativeFlowBindings | null {
	const { plan, invocation, evidence, layout, fixture } = options;
	if (invocation.journey.journeyId !== 'quota-group-by-f6') return null;
	const base = quotaBaseBindings({ plan, invocation, evidence, layout });
	const inputs = new Map<number, NativeFlowInputBinding>();
	let quits = 0;
	for (const [index, step] of invocation.journey.steps.entries())
		if (step.kind === 'press') {
			if (step.key === 'Ctrl+C' && quits++ === 0)
				inputs.set(index, {
					kind: 'handoff',
					expected: base.quit.expected,
					handoff: async (context) => {
						const access = await fixture.access(),
							live = await fixture.read(context.frame.binding.sequence);
						return quotaGroupingHandoff({
							...access,
							afterSequence: access.journal.read().length,
							previousMount: live.readback.mount,
							storePath: join(access.started.routerRoot, 'agent-proxy-ui.sqlite'),
							ready: base.ready,
							firstFrame: base.initialBarrier,
						});
					},
				});
			else
				inputs.set(index, {
					kind: step.key === 'Ctrl+C' ? 'exit' : 'continuing',
					expected:
						step.key === 'Ctrl+C'
							? base.quit.expected
							: [
									quotaExpectedKey(
										step.key,
										quotaScreen,
										null,
										step.key === 'F6' ? 'quota:group-cycle' : null,
									),
								],
				});
		}
	let currentFacet: 'none' | 'provider' | 'status' = 'none',
		currentRows: readonly string[] = [],
		menu: QuotaMenuObservation | null = null;
	let baseline: QuotaLiveReadback['readback']['rows'] | null = null,
		latestMount = 0,
		nextOverlay = 1;
	const inspect = async (
		context: NativeFlowContext,
		facet: typeof currentFacet,
	): Promise<MarkedInputEvidence> => {
		const live = await fixture.read(context.frame.binding.sequence);
		if (latestMount !== live.readback.mount) {
			latestMount = live.readback.mount;
			nextOverlay = 1;
		}
		baseline ??= live.readback.rows;
		currentFacet = facet;
		let marked = await context.input.press('F12', [quotaExpectedKey('F12', quotaScreen)]),
			moved = 0;
		const census: string[] = [...gridLines(quotaMarkedFrame(marked).grid)];
		const advance = async (count: number): Promise<void> => {
			const before = gridLines(quotaMarkedFrame(marked).grid);
			marked = await context.input.press('Alt+Down', [
				quotaExpectedKey('Alt+Down', quotaScreen, 'action:quota:accounts'),
			]);
			const after = gridLines(quotaMarkedFrame(marked).grid);
			if (JSON.stringify(before) === JSON.stringify(after)) return;
			moved += 1;
			census.push(...after);
			if (count === 255)
				throw new Error('quota grouping census did not reach the physical content end');
			await advance(count + 1);
		};
		await advance(0);
		await Array.from({ length: moved }).reduce(async (previous) => {
			await previous;
			marked = await context.input.press('Alt+Up', [
				quotaExpectedKey('Alt+Up', quotaScreen, 'action:quota:accounts'),
			]);
		}, Promise.resolve());
		currentRows = census;
		marked = await context.input.press('F10', [
			quotaExpectedKey('F10', quotaScreen, null, 'quota:menu'),
		]);
		const surface: InputConsumptionExpectation['surface'] = quotaOverlay(nextOverlay++);
		const collected = await collectQuotaMenu({
			context: { ...context, frame: quotaMarkedFrame(marked) },
			surface,
		});
		menu = collected.observation;
		marked = await context.input.press('Escape', [quotaExpectedKey('Escape', surface)]);
		writeFileSync(
			join(evidence, 'grouping-' + latestMount + '-' + facet + '.json'),
			JSON.stringify(
				{
					facet,
					rows: currentRows,
					menu,
					inputSource: live,
					driverPid: context.driver.processId(),
				},
				null,
				2,
			),
		);
		return marked;
	};
	const check = async (name: FlowCheck, context: NativeFlowContext): Promise<readonly string[]> => {
		const pose = (await fixture.read(context.frame.binding.sequence)).readback;
		if (name === 'quota-grouping-selection')
			return pose.listSelection === 'effect-account-0'
				? []
				: ['grouping changed the independently selected identity'];
		if (name === 'quota-grouping-checked') {
			const checked = menu?.items.filter((item) => item.checked).map((item) => item.actionId);
			return JSON.stringify(checked) === JSON.stringify(['group-' + currentFacet])
				? []
				: ['painted menu grouping check differs from the selected facet'];
		}
		if (name === 'quota-grouping-buckets') {
			if (baseline === null) throw new Error('grouping baseline was not observed');
			const text = currentRows.join('\n'),
				failures: string[] = [];
			for (const label of ['effect-alpha', 'effect-beta'])
				if (!text.includes(label)) failures.push('physical grouped account missing: ' + label);
			const groups = new Map<string, number>();
			if (currentFacet !== 'none')
				for (const row of baseline) {
					const bucketName =
						currentFacet === 'provider'
							? row.provider === 'openai'
								? 'OpenAI'
								: 'Claude'
							: statusHeading(row.status);
					groups.set(bucketName, (groups.get(bucketName) ?? 0) + 1);
				}
			for (const [heading, count] of groups)
				if (!text.includes(heading + ' (' + count + ')'))
					failures.push('physical group heading/count missing: ' + heading);
			if (
				currentFacet === 'none' &&
				/(?:OpenAI|Claude|Preferred|Available|Held|Blocked|Stale|Credentials unavailable) \(\d+\)/u.test(
					text,
				)
			)
				failures.push('None retained a group heading');
			return failures;
		}
		throw new Error('unbound quota grouping check: ' + name);
	};
	const checkNames: readonly FlowCheck[] = [
		'quota-grouping-buckets',
		'quota-grouping-selection',
		'quota-grouping-checked',
	];
	return {
		...base,
		inputs,
		fixtures: new Map(
			(['none', 'provider', 'status'] as const).map((facet) => [
				'inspect-quota-grouping-' + facet,
				async (context: NativeFlowContext) => inspect(context, facet),
			]),
		),
		readbacks: new Map([['quota-preferences', async () => fixture.verifyFinal()]]),
		checks: new Map([
			...checkNames.map(
				(name) => [name, async (context: NativeFlowContext) => check(name, context)] as const,
			),
			[
				'quota-grouping-persistence',
				async (_context: NativeFlowContext, value: unknown) =>
					checkQuotaEffectReceipt({
						scenario: 'grouping-reopen',
						routerRoot: (await fixture.access()).started.routerRoot,
						value,
					}),
			] as const,
			[
				'no-retry',
				async (_context: NativeFlowContext, value: unknown) => {
					const receipt = quotaEffectReceiptSchema.parse(value);
					return receipt.loginChild === null &&
						receipt.accountCommands.length === 0 &&
						receipt.unexpectedEffectCalls.length === 0
						? []
						: ['unexpected grouping child or account effect'];
				},
			] as const,
		]),
	};
}
