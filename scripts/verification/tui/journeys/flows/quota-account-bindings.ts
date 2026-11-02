/** Captured-account actions and held removal stay on the real CLI runner and one PTY. */
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { z } from 'zod';

import type { InputConsumptionExpectation } from '../../sessions/input-consumption-schema.ts';
import type { MarkedInputEvidence } from '../../terminal/marker-input-session.ts';
import { gridLines } from '../../terminal/terminal-grid.ts';
import { findDialog } from '../scenes/grid-scene-matcher.ts';
import type { AuthoredFlowPlan } from './authored-flow-plan.ts';
import type { FlowCheck } from './flow-definition.ts';
import type { FlowInvocation } from './flow-executor.ts';
import type {
	NativeFlowBindings,
	NativeFlowContext,
	NativeFlowInputBinding,
} from './native-flow-runtime.ts';
import { locatePaintedTarget } from './painted-flow-target.ts';
import { checkQuotaAccount, quotaRemoveCompletionMessage } from './quota-account-checks.ts';
import { quotaAccountFactSchema, quotaEffectReceiptSchema } from './quota-effect-readback.ts';
import { quotaCapturedPosition, quotaJourneyScenarios } from './quota-fixture-contract.ts';
import type { QuotaFixtureLayout } from './quota-fixture-layout.ts';
import {
	quotaBaseBindings,
	quotaDialogTarget,
	quotaExpectedKey,
	quotaFocused,
	quotaMarkedFrame,
	quotaMouse,
	quotaOverlay,
	quotaScreen,
} from './quota-flow-inputs.ts';
import type { QuotaLiveFixture } from './quota-live-fixture.ts';
import type { quotaObservationSchema } from './quota-live-fixture.ts';

const ownerFinishedSchema = z.strictObject({
	kind: z.literal('account-owner-finished'),
	command: quotaEffectReceiptSchema.shape.accountCommands.element.shape.command,
	result: quotaEffectReceiptSchema.shape.accountCommands.element.shape.result,
	accounts: z.array(quotaAccountFactSchema),
});
const heldSchema = z
	.object({ kind: z.enum(['removal-before-owner-held', 'removal-after-credential-delete-held']) })
	.strict();
const accountChoices = [
	['login', 'Log in again'],
	['options', 'Options…'],
	['floor', 'Weekly floor…'],
	['account-status', 'Disable'],
	['copy-id', 'Copy account ID'],
	['remove', 'Remove account…'],
] as const;
type Surface = InputConsumptionExpectation['surface'];
interface AccountSelectionStep {
	readonly surface: Surface;
	readonly accountId: string;
	readonly moveBrowse?: boolean;
}
type FixtureStep =
	| ({ readonly kind: 'open' } & AccountSelectionStep)
	| ({ readonly kind: 'select' } & AccountSelectionStep)
	| ({ readonly kind: 'gone' } & AccountSelectionStep)
	| {
			readonly kind: 'reveal' | 'menu' | 'try-unavailable';
			readonly surface: Surface;
			readonly enabled: boolean;
			readonly label?: string;
	  }
	| {
			readonly kind: 'settle';
			readonly surface: Surface;
			readonly action: 'copy-id' | 'disable' | 'enable' | 'remove' | null;
	  };

export function quotaAccountBindings(options: {
	readonly plan: AuthoredFlowPlan;
	readonly invocation: FlowInvocation;
	readonly evidence: string;
	readonly layout: QuotaFixtureLayout;
	readonly fixture: QuotaLiveFixture;
}): NativeFlowBindings | null {
	const { plan, invocation, evidence, layout, fixture } = options,
		scenario = layout.configuration.scenario;
	if (
		![
			'captured-actions',
			'remove-cancel',
			'remove-middle',
			'remove-last',
			'remove-only',
			'remove-held',
			'remove-requires-disabled',
		].includes(scenario)
	)
		return null;
	if (!quotaJourneyScenarios(invocation.journey.journeyId).includes(scenario))
		throw new Error('account scenario differs from authored journey');
	const base = quotaBaseBindings({ plan, invocation, evidence, layout }),
		inputs = new Map<number, NativeFlowInputBinding>();
	let active: Surface = quotaScreen,
		dialog: Surface = quotaScreen,
		nextOverlay = 1,
		enabled = scenario === 'captured-actions',
		dialogKind: 'details' | 'options' | 'floor' = 'details',
		lastAction: FixtureStep & { kind: 'settle' } = {
			kind: 'settle',
			surface: quotaScreen,
			action: null,
		};
	const position = quotaCapturedPosition(scenario),
		accountId = 'effect-account-' + position;
	const operations = new Map<string, FixtureStep[]>();
	const add = (name: string, spec: FixtureStep): void => {
		const queue = operations.get(name) ?? [];
		queue.push(spec);
		operations.set(name, queue);
	};
	for (const [index, step] of invocation.journey.steps.entries()) {
		if (step.kind === 'fixture') {
			if (step.operation.startsWith('open-details-for-')) {
				active = quotaOverlay(nextOverlay++);
				dialog = active;
				dialogKind = 'details';
				add(step.operation, {
					kind: 'open',
					surface: active,
					accountId,
					moveBrowse: scenario === 'captured-actions',
				});
			} else if (step.operation.startsWith('disabled-remove-'))
				add(step.operation, { kind: 'select', surface: quotaScreen, accountId });
			else if (step.operation === 'remove-captured-a-from-reloaded-report')
				add(step.operation, { kind: 'gone', surface: dialog, accountId });
			else if (step.operation === 'publish-settled-account-command') {
				const removalSucceeded: boolean =
					lastAction.action === 'remove' && scenario !== 'remove-requires-disabled';
				active = removalSucceeded ? quotaScreen : dialog;
				add(step.operation, { ...lastAction, surface: active });
			} else if (step.operation === 'try-unavailable-account-remove')
				add(step.operation, { kind: 'try-unavailable', surface: active, enabled });
			else if (step.operation === 'inspect-account-actions-menu')
				add(step.operation, { kind: 'menu', surface: active, enabled });
			else if (step.operation.startsWith('reveal-quota-account-'))
				add(step.operation, {
					kind: 'reveal',
					surface: active,
					enabled,
					label: step.operation.slice('reveal-quota-account-'.length),
				});
			else throw new Error('unbound account fixture ' + step.operation);
		} else if (step.kind === 'press') {
			if (step.key === 'Enter' && active.kind === 'screen') {
				inputs.set(index, {
					kind: 'continuing',
					expected: [
						quotaExpectedKey('Enter', quotaScreen, 'action:quota:accounts', 'quota:details'),
					],
				});
				active = quotaOverlay(nextOverlay++);
				dialog = active;
				dialogKind = 'details';
			} else if (step.key === 'Enter') {
				inputs.set(index, {
					kind: 'continuing',
					expected: [
						quotaExpectedKey(
							'Enter',
							active,
							'action:quota:confirm-remove',
							'quota:confirm-remove',
						),
					],
				});
				lastAction = { kind: 'settle', surface: active, action: 'remove' };
			} else if (step.key === 'Ctrl+Enter')
				inputs.set(index, {
					kind: 'continuing',
					expected: [quotaExpectedKey('Ctrl+Enter', active, null, 'quota:cancel-remove')],
				});
			else if (step.key === 'Escape') {
				inputs.set(index, {
					kind: 'continuing',
					expected: [
						quotaExpectedKey(
							'Escape',
							active,
							'action:quota:' + (dialogKind === 'options' ? 'domain-body' : 'floor-decrease'),
						),
					],
				});
				active = quotaScreen;
			} else throw new Error('unbound account key ' + step.key);
		} else if (step.kind === 'focus')
			inputs.set(index, {
				kind: 'focus',
				stopWhenFocused: true,
				route: Array.from({ length: 4 }, () => ({
					key: 'Tab',
					expected: [quotaExpectedKey('Tab', active)],
				})),
				isFocused: (grid) => quotaFocused(grid, 'Remove account', 'Remove'),
			});
		else if (step.kind === 'click') {
			const before = active;
			let action: string;
			if (step.target === 'Actions ▾') {
				action = 'account-actions';
				active = quotaOverlay(nextOverlay++);
			} else {
				const selected =
					step.target === 'Enable'
						? 'account-status'
						: accountChoices.find(([, label]) => label === step.target)?.[0];
				if (selected === undefined) throw new Error('unknown captured action ' + step.target);
				action = selected;
				if (selected === 'options' || selected === 'floor') {
					active = quotaOverlay(nextOverlay++);
					dialog = active;
					dialogKind = selected;
				} else active = dialog;
				if (selected === 'account-status') {
					lastAction = { kind: 'settle', surface: active, action: enabled ? 'disable' : 'enable' };
					enabled = !enabled;
				} else if (selected === 'copy-id')
					lastAction = { kind: 'settle', surface: active, action: 'copy-id' };
			}
			const after = active;
			inputs.set(index, {
				kind: 'click',
				ending: 'continue',
				expected: (location) => [
					quotaMouse(location, 'down', before, 'action:quota:' + action, 'quota:' + action),
					quotaMouse(location, 'up', after, null, null),
				],
			});
		}
	}
	let ownerBoundary = 0,
		expectedCommands = 0;
	const observe = async (
		context: NativeFlowContext,
	): Promise<z.infer<typeof quotaObservationSchema>> => {
		const access = await fixture.access();
		return {
			live: await fixture.read(context.frame.binding.sequence),
			events: [...access.journal.read()],
			routerRoot: access.started.routerRoot,
		};
	};
	const runFixture = async (
		context: NativeFlowContext,
		spec: FixtureStep,
	): Promise<MarkedInputEvidence> => {
		if (spec.kind === 'open' || spec.kind === 'select') {
			const live = await fixture.read(context.frame.binding.sequence);
			await fixture.setup({
				operation: 'select-browse-account',
				mount: live.readback.mount,
				accountId: spec.accountId,
			});
			let marked = await context.input.press('F12', [quotaExpectedKey('F12', quotaScreen)]);
			if (spec.kind === 'open') {
				marked = await context.input.press('Enter', [
					quotaExpectedKey('Enter', quotaScreen, 'action:quota:accounts', 'quota:details'),
				]);
				if (spec.moveBrowse) {
					await fixture.setup({
						operation: 'select-browse-account',
						mount: live.readback.mount,
						accountId: 'effect-account-1',
					});
					await fixture.setup({ operation: 'reload-view-model', mount: live.readback.mount });
					marked = await context.input.press('F12', [quotaExpectedKey('F12', spec.surface)]);
				}
			}
			ownerBoundary = fixture.boundary();
			return marked;
		}
		if (spec.kind === 'gone') {
			const live = await fixture.read(context.frame.binding.sequence);
			await fixture.setup({
				operation: 'remove-captured-account-externally',
				mount: live.readback.mount,
			});
			return context.input.press('F12', [quotaExpectedKey('F12', spec.surface)]);
		}
		if (spec.kind === 'settle') {
			if (spec.action !== null) {
				// This caller owns the same 8s failure bound as existing native input phases; time never makes a success.
				const signal = AbortSignal.timeout(8000);
				if (scenario === 'remove-held' || scenario === 'remove-requires-disabled') {
					const kind =
						scenario === 'remove-held'
							? 'removal-after-credential-delete-held'
							: 'removal-before-owner-held';
					await fixture.event({
						afterSequence: ownerBoundary,
						kind,
						signal,
						decode: (value) => heldSchema.parse(value),
					});
					await fixture.setup(
						scenario === 'remove-held'
							? { operation: 'release-removal' }
							: { operation: 'reenable-before-removal' },
					);
				}
				const completed = await fixture.event({
					afterSequence: ownerBoundary,
					kind: 'account-owner-finished',
					signal,
					decode: (value) => ownerFinishedSchema.parse(value),
				});
				if (
					completed.event.command.action !== spec.action ||
					completed.event.command.accountId !== accountId
				)
					throw new Error('actual account completion retargeted or changed the requested action');
				ownerBoundary = completed.sequence;
				expectedCommands += 1;
				fixture.invalidate();
				if (spec.surface.kind === 'screen')
					await context.driver.waitForGrid(
						'actual removal return',
						(grid) =>
							findDialog(grid, 'Account details') === null &&
							findDialog(grid, 'Remove account') === null &&
							gridLines(grid)
								.join(' ')
								.replace(/\s+/gu, ' ')
								.includes(
									quotaRemoveCompletionMessage('effect-' + ['alpha', 'beta', 'gamma'][position]),
								),
					);
				else if (spec.action === 'disable')
					await context.driver.waitForGrid('actual disabled Details', (grid) =>
						gridLines(grid).join(' ').toLowerCase().includes('disabled'),
					);
				else if (spec.action === 'enable')
					await context.driver.waitForGrid(
						'actual enabled Details',
						(grid) => !gridLines(grid).join(' ').toLowerCase().includes('disabled'),
					);
			}
			return context.input.press('F12', [quotaExpectedKey('F12', spec.surface)]);
		}
		if (spec.kind === 'try-unavailable') {
			const target = quotaDialogTarget(context.frame.grid, '', 'Remove account…');
			const location = locatePaintedTarget(context.frame.grid, {
				label: target.label,
				bounds: target.bounds,
			});
			return context.input.observe({
				expected: [
					quotaMouse(location, 'down', spec.surface, null, null),
					quotaMouse(location, 'up', spec.surface, null, null),
				],
				inject: async () => {
					const sent = await context.input.input.click(location.column, location.row);
					if (sent.some((record) => record.delivery.kind !== 'sent'))
						throw new Error('unavailable Remove probe was not delivered');
				},
			});
		}
		const choices = accountChoices.map(([actionId, label]) => ({
			actionId,
			label: actionId === 'account-status' ? (spec.enabled ? 'Disable' : 'Enable') : label,
		}));
		const eligible = choices.filter((choice) =>
			spec.enabled ? choice.actionId !== 'remove' : true,
		);
		let marked = await context.input.press('F12', [quotaExpectedKey('F12', spec.surface)]);
		const seen = new Set<string>();
		const inspect = (): void => {
			const text = gridLines(quotaMarkedFrame(marked).grid).join('\n');
			for (const choice of choices) if (text.includes(choice.label)) seen.add(choice.actionId);
		};
		inspect();
		const reveal = async (index: number): Promise<void> => {
			if (spec.kind === 'reveal')
				try {
					locatePaintedTarget(
						quotaMarkedFrame(marked).grid,
						quotaDialogTarget(quotaMarkedFrame(marked).grid, '', spec.label ?? ''),
					);
					return;
				} catch {
					/* Continue only by declared, acknowledged focus navigation. */
				}
			if (index >= eligible.length * 2) {
				if (spec.kind === 'reveal')
					throw new Error('account menu target was not revealed: ' + spec.label);
				return;
			}
			const previous = eligible[index % eligible.length];
			if (previous === undefined) throw new Error('account menu inventory empty');
			marked = await context.input.press('Down', [
				quotaExpectedKey('Down', spec.surface, 'action:quota:' + previous.actionId),
			]);
			inspect();
			await reveal(index + 1);
		};
		await reveal(0);
		if (spec.kind === 'menu' && seen.size !== choices.length)
			throw new Error('closed account Actions menu omitted a painted item');
		writeFileSync(
			join(evidence, 'account-menu-' + marked.observation.sequence + '.json'),
			JSON.stringify(
				{ seen: [...seen], source: await observe({ ...context, frame: quotaMarkedFrame(marked) }) },
				null,
				2,
			),
		);
		return marked;
	};
	const fixtures = new Map<
		string,
		(context: NativeFlowContext) => Promise<MarkedInputEvidence | null>
	>();
	for (const [name, queue] of operations)
		fixtures.set(name, async (context) => {
			const spec = queue.shift();
			if (spec === undefined) throw new Error('account fixture repeated beyond authored steps');
			return runFixture(context, spec);
		});
	const checkNames: FlowCheck[] = [
		'account-actions-inventory',
		'captured-account-target',
		'options-and-floor-captured-target',
		'clipboard-full-account-id',
		'status-toggle',
		'cancel-primary-and-no-remove',
		'explicit-remove-and-reselection',
		'disabled-first-refusal',
		'no-retry',
	];
	const labels = new Set(
		invocation.journey.steps.flatMap((step) =>
			step.kind === 'click' || step.kind === 'focus' ? [step.target] : [],
		),
	);
	return {
		...base,
		inputs,
		fixtures,
		targets: new Map(
			[...labels].map((label) => [
				label,
				(grid) => quotaDialogTarget(grid, label === 'Remove' ? 'Remove account' : '', label),
			]),
		),
		readbacks: new Map([
			['account-command', observe],
			['clipboard', observe],
		]),
		checks: new Map(
			checkNames.map((check) => [
				check,
				async (context, value) =>
					checkQuotaAccount({
						check,
						scenario,
						grid: context.frame.grid,
						observation: value === undefined ? await observe(context) : value,
						expectedCommands,
					}),
			]),
		),
	};
}
