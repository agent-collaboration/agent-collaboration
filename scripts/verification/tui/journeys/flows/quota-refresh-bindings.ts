/** One held real service/mapper invocation; fresh key/menu children are owned by quota-case-run. */
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { z } from 'zod';

import type { MarkedInputEvidence } from '../../terminal/marker-input-session.ts';
import { renderOriginal } from '../../terminal/terminal-original.ts';
import type { AuthoredFlowPlan } from './authored-flow-plan.ts';
import type { FlowCheck } from './flow-definition.ts';
import type { FlowInvocation } from './flow-executor.ts';
import type {
	NativeFlowBindings,
	NativeFlowContext,
	NativeFlowInputBinding,
} from './native-flow-runtime.ts';
import { locatePaintedTarget } from './painted-flow-target.ts';
import { collectQuotaContent } from './quota-content-census.ts';
import type { QuotaFixtureLayout } from './quota-fixture-layout.ts';
import {
	collectQuotaDialog,
	quotaBaseBindings,
	quotaDialogTarget,
	quotaExpectedKey,
	quotaMarkedFrame,
	quotaMouse,
	quotaOverlay,
	quotaScreen,
} from './quota-flow-inputs.ts';
import type { QuotaLiveFixture, QuotaLiveReadback } from './quota-live-fixture.ts';
import {
	collectQuotaMenu,
	currentQuotaMenu,
	quotaMenuNavigationExpected,
} from './quota-menu-observation.ts';
import { checkQuotaRefresh } from './quota-refresh-checks.ts';
import {
	quotaRefreshPublicationSchema,
	requireQuotaRefreshPublication,
} from './quota-refresh-publication.ts';
const publishedFrameSchema = z.object({
	frame: z.number().int().nonnegative().max(Number.MAX_SAFE_INTEGER),
});
export function quotaRefreshBindings(options: {
	readonly plan: AuthoredFlowPlan;
	readonly invocation: FlowInvocation;
	readonly evidence: string;
	readonly layout: QuotaFixtureLayout;
	readonly fixture: QuotaLiveFixture;
}): NativeFlowBindings | null {
	const { plan, invocation, evidence, layout, fixture } = options,
		scenario = layout.configuration.scenario;
	if (!scenario.startsWith('refresh-')) return null;
	const base = quotaBaseBindings({ plan, invocation, evidence, layout }),
		inputs = new Map<number, NativeFlowInputBinding>();
	const menuRoute = !invocation.journey.steps.some(
		(step) => step.kind === 'press' && step.key === 'R',
	);
	let phase: 'before' | 'pending' | 'completed' = 'before',
		baseline: QuotaLiveReadback | null = null,
		boundary = 0,
		active = quotaScreen as ReturnType<typeof quotaOverlay> | typeof quotaScreen,
		body: readonly string[] = [];
	let refreshPresses = 0;
	for (const [index, step] of invocation.journey.steps.entries()) {
		if (step.kind === 'press')
			inputs.set(index, {
				kind: 'continuing',
				expected: [
					step.key === 'F10'
						? quotaExpectedKey('F10', quotaScreen, null, 'quota:menu')
						: quotaExpectedKey(
								'R',
								quotaScreen,
								null,
								refreshPresses++ === 0 ? 'quota:refresh' : null,
							),
				],
			});
		if (step.kind === 'click')
			inputs.set(index, {
				kind: 'click',
				ending: 'continue',
				expected: (location) => [
					quotaMouse(location, 'down', quotaOverlay(1), null, null),
					quotaMouse(location, 'up', quotaOverlay(1), null, null),
				],
			});
	}
	const initialize = async (context: NativeFlowContext): Promise<void> => {
		baseline = await fixture.read(context.frame.binding.sequence);
		boundary = fixture.boundary();
		if (baseline.readback.listSelection !== 'effect-account-0')
			throw new Error('refresh initial selected identity differs');
	};
	const read = async (context: NativeFlowContext): Promise<unknown> => {
		fixture.invalidate();
		const access = await fixture.access();
		return {
			live: await fixture.read(context.frame.binding.sequence),
			events: access.journal.read(),
			routerRoot: access.started.routerRoot,
		};
	};
	const held = async (context: NativeFlowContext): Promise<MarkedInputEvidence> => {
		const kind =
			scenario === 'refresh-setup-failed' ? 'refresh-setup-held' : 'refresh-first-request-held';
		await fixture.event({
			afterSequence: boundary,
			kind,
			signal: AbortSignal.timeout(8000),
			decode: (value) => z.object({ kind: z.literal(kind) }).parse(value),
		});
		phase = 'pending';
		fixture.invalidate();
		return context.input.press('F12', [quotaExpectedKey('F12', active)]);
	};
	const release = async (context: NativeFlowContext): Promise<MarkedInputEvidence> => {
		if (baseline === null) throw new Error('refresh baseline absent');
		if (!menuRoute) {
			await context.input.press('Escape', [quotaExpectedKey('Escape', quotaOverlay(1))]);
			active = quotaScreen;
		}
		await fixture.setup({ operation: 'release-refresh' });
		const event = await fixture.event({
			afterSequence: boundary,
			kind: 'refresh-result-published',
			signal: AbortSignal.timeout(8000),
			decode: (value) => quotaRefreshPublicationSchema.parse(value),
		});
		fixture.invalidate();
		const live = await fixture.read(context.frame.binding.sequence),
			mapped = live.refresh?.observations.find((value) => value.kind === 'mapping-returned');
		if (mapped?.kind !== 'mapping-returned')
			throw new Error('refresh mapper return absent after publication');
		requireQuotaRefreshPublication(event.event, {
			mount: baseline.readback.mount,
			afterFrame: publishedFrameSchema.parse(baseline.readback.published).frame,
			result: mapped.result,
		});
		phase = 'completed';
		let marked = await context.input.press('F12', [quotaExpectedKey('F12', active)]);
		if (menuRoute) {
			const collected = await collectQuotaDialog({
				context: { ...context, frame: quotaMarkedFrame(marked) },
				title: 'Account details',
				surface: active,
				focusedTarget: 'action:quota:account-actions',
			});
			marked = collected.marked;
			body = collected.rows;
		} else {
			const collected = await collectQuotaContent({ ...context, frame: quotaMarkedFrame(marked) });
			marked = collected.marked;
			body = collected.full;
		}
		writeFileSync(
			join(evidence, 'refresh-publication.json'),
			JSON.stringify({ event, body }, null, 2),
		);
		return marked;
	};
	const inspectMenu = async (context: NativeFlowContext): Promise<MarkedInputEvidence> => {
		const collected = await collectQuotaMenu({
			context,
			surface: quotaOverlay(1),
			disabled: ['refresh'],
		});
		active = quotaOverlay(1);
		return collected.marked;
	};
	const runMenu = async (context: NativeFlowContext): Promise<MarkedInputEvidence> => {
		await initialize(context);
		let marked = await context.input.press('F10', [
			quotaExpectedKey('F10', quotaScreen, null, 'quota:menu'),
		]);
		active = quotaOverlay(1);
		const reveal = async (index: number): Promise<void> => {
			try {
				locatePaintedTarget(
					quotaMarkedFrame(marked).grid,
					quotaDialogTarget(quotaMarkedFrame(marked).grid, '', 'Refresh'),
				);
				return;
			} catch {
				if (index >= currentQuotaMenu.length * 2)
					throw new Error('real Refresh menu label not revealed');
			}
			marked = await context.input.press('Down', [
				quotaMenuNavigationExpected('Down', index, active),
			]);
			await reveal(index + 1);
		};
		await reveal(0);
		const location = locatePaintedTarget(
			quotaMarkedFrame(marked).grid,
			quotaDialogTarget(quotaMarkedFrame(marked).grid, '', 'Refresh'),
		);
		marked = await context.input.observe({
			expected: [
				quotaMouse(location, 'down', active, 'action:quota:refresh', 'quota:refresh'),
				quotaMouse(location, 'up', quotaScreen, null, null),
			],
			inject: async () => {
				const records = await context.input.input.click(location.column, location.row);
				if (records.some((record) => record.delivery.kind !== 'sent'))
					throw new Error('Refresh menu input not sent');
			},
		});
		active = quotaScreen;
		marked = await held({ ...context, frame: quotaMarkedFrame(marked) });
		marked = await context.input.press('Enter', [
			quotaExpectedKey('Enter', quotaScreen, 'action:quota:accounts', 'quota:details'),
		]);
		active = quotaOverlay(2);
		const frame = quotaMarkedFrame(marked);
		await renderOriginal({
			...base.original,
			output: evidence,
			checkpoint: 'menu-refresh-pending-details',
			columns: frame.grid.columns,
			rows: frame.grid.rows,
			bytes: frame.bytes,
			grid: frame.grid,
			resizes: frame.resizes,
		});
		return release({ ...context, frame });
	};
	const fixtures = new Map<
		string,
		(context: NativeFlowContext) => Promise<MarkedInputEvidence | null>
	>([
		[
			'refresh-' + scenario.slice('refresh-'.length),
			async (context) => {
				await initialize(context);
				return null;
			},
		],
		[
			'hold-first-refresh',
			async (context) => {
				const live = await fixture.read(context.frame.binding.sequence),
					refresh = live.refresh,
					action = live.readback.actions.find((item) => item.id === 'quota:refresh');
				if (
					refresh === null ||
					refresh.serviceCalls !== 0 ||
					refresh.released ||
					action?.available !== true
				)
					throw new Error('fresh refresh gate was not armed before input');
				return null;
			},
		],
		['await-refresh-held', held],
		['inspect-pending-refresh-menu', inspectMenu],
		['release-first-refresh', release],
		['repeat-through-menu-with-details-open', runMenu],
	]);
	const checkNames: FlowCheck[] = [
		'refresh-one-in-flight',
		'refresh-visible-and-disabled',
		'refresh-values-and-details',
		'typed-refresh-result',
		'no-retry',
	];
	return {
		...base,
		inputs,
		fixtures,
		targets: new Map([
			[
				'Refresh',
				(grid) => {
					const target = quotaDialogTarget(grid, '', 'Refresh');
					return { label: target.label, bounds: target.bounds };
				},
			],
		]),
		readbacks: new Map([['quota-refresh', read]]),
		checks: new Map(
			checkNames.map((check) => [
				check,
				async (context, value) => {
					if (baseline === null) throw new Error('refresh baseline absent');
					return checkQuotaRefresh({
						check,
						scenario,
						phase,
						baseline,
						observation: value === undefined ? await read(context) : value,
						grid: context.frame.grid,
						body,
					});
				},
			]),
		),
	};
}
