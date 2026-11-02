/** Read-only compiled quota component: toolbar, full menus, Help and same-child resize. */
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';

import type { ProcessIdentity } from '../../evidence/evidence-schema.ts';
import {
	lookupProcessIdentity,
	requireOwnedProductChild,
	sameIdentity,
} from '../../processes/process-identity.ts';
import type { InputConsumptionExpectation } from '../../sessions/input-consumption-schema.ts';
import type { MarkedInputEvidence } from '../../terminal/marker-input-session.ts';
import { cellText, terminalTheme } from '../../terminal/terminal-grid.ts';
import { renderOriginal } from '../../terminal/terminal-original.ts';
import { journeyGridSizes } from '../journey-definition.ts';
import { findDialog } from '../scenes/grid-scene-matcher.ts';
import { checkNoBottomActions } from '../scenes/public-entry-scene-checks.ts';
import type { AuthoredFlowPlan } from './authored-flow-plan.ts';
import type { FlowInvocation } from './flow-executor.ts';
import type {
	NativeFlowBindings,
	NativeFlowContext,
	NativeFlowInputBinding,
} from './native-flow-runtime.ts';
import {
	collectQuotaDialog,
	quotaExpectedKey,
	quotaMarkedFrame,
	quotaMouse,
	quotaOverlay,
	quotaScreen,
} from './quota-flow-inputs.ts';
import { collectQuotaMenu } from './quota-menu-observation.ts';
import type { QuotaMenuObservation } from './quota-menu-observation.ts';
import {
	checkQuotaHelpRows,
	checkQuotaToolbar,
	quotaListFocused,
	quotaToolbarFocus,
	quotaToolbarRow,
} from './quota-toolbar-observation.ts';

export function quotaToolbarBindings(
	plan: AuthoredFlowPlan,
	invocation: FlowInvocation,
	evidence: string,
): NativeFlowBindings | null {
	if (invocation.journey.journeyId !== 'quota-toolbar-and-menu') return null;
	const inputs = new Map<number, NativeFlowInputBinding>();
	let active: InputConsumptionExpectation['surface'] = quotaScreen,
		nextOverlay = 1;
	for (const [index, step] of invocation.journey.steps.entries()) {
		if (step.kind === 'press') {
			const before = active;
			let target: string | null = null,
				action: string | null = null;
			if (step.key === 'F10' || step.key === 'Enter') {
				target = step.key === 'Enter' ? 'action:quota:menu' : null;
				action = 'quota:menu';
				active = quotaOverlay(nextOverlay++);
			} else if (step.key === 'F1') {
				action = 'quota:help';
				active = quotaOverlay(nextOverlay++);
			} else if (step.key === 'Escape') {
				target =
					before.kind === 'overlay' && before.overlayId === '3'
						? '17:action:quota:help:help-body'
						: null;
				active = quotaScreen;
			} else throw new Error('quota toolbar key is not declared: ' + step.key);
			inputs.set(index, {
				kind: 'continuing',
				expected: [quotaExpectedKey(step.key, before, target, action)],
			});
		} else if (step.kind === 'click') {
			if (step.target !== '≡ Menu') throw new Error('quota toolbar click is not declared');
			const before = active,
				after = quotaOverlay(nextOverlay++);
			active = after;
			inputs.set(index, {
				kind: 'click',
				ending: 'continue',
				expected: (location) => [
					quotaMouse(location, 'down', before, 'action:quota:menu', 'quota:menu'),
					quotaMouse(location, 'up', after, null, null),
				],
			});
		} else if (step.kind === 'focus') {
			const key = step.via === 'Shift+Tab' ? 'Shift+Tab' : 'Tab';
			inputs.set(index, {
				kind: 'focus',
				stopWhenFocused: true,
				route: Array.from({ length: 12 }, () => ({
					key,
					expected: [quotaExpectedKey(key, quotaScreen)],
				})),
				isFocused: (grid) =>
					step.target === 'main list' ? quotaListFocused(grid) : quotaToolbarFocus(grid, 'Menu'),
			});
		}
	}
	let child: ProcessIdentity | null = null,
		menu: QuotaMenuObservation | null = null,
		helpQualified = false,
		resizeQualified = false;
	const requireChild = (context: NativeFlowContext): void => {
		child ??= requireOwnedProductChild(context.driver.processId(), plan.product.path);
		if (!sameIdentity(lookupProcessIdentity(child.pid), child))
			throw new Error('quota toolbar lost the exact product identity');
	};
	const original = {
		chrome: plan.chrome,
		sourceHead: plan.productSource.head,
		binarySha256: plan.product.sha256,
	};
	const fixtures = new Map<
		string,
		(context: NativeFlowContext) => Promise<MarkedInputEvidence | null>
	>();
	for (const [checkpoint, ordinal] of [
		['menu-f10', 1],
		['menu-from-click', 2],
		['menu-from-tab', 4],
		['narrow-menu-complete', 5],
	] as const) {
		fixtures.set('inspect-quota-menu-' + checkpoint, async (context: NativeFlowContext) => {
			requireChild(context);
			const collected = await collectQuotaMenu({ context, surface: quotaOverlay(ordinal) });
			menu = collected.observation;
			writeFileSync(join(evidence, checkpoint + '-inventory.json'), JSON.stringify(menu, null, 2));
			return collected.marked;
		});
	}
	fixtures.set('inspect-quota-menu-help', async (context: NativeFlowContext) => {
		requireChild(context);
		const result = await collectQuotaDialog({
			context,
			title: 'Help',
			surface: quotaOverlay(3),
			focusedTarget: '17:action:quota:help:help-body',
		});
		const failures = checkQuotaHelpRows(result.rows);
		writeFileSync(
			join(evidence, 'quota-help-census.json'),
			JSON.stringify({ rows: result.rows, failures }, null, 2),
		);
		if (failures.length > 0) throw new Error(failures.join('; '));
		helpQualified = true;
		return result.marked;
	});
	fixtures.set('resize-same-child-to-each-declared-grid', async (context: NativeFlowContext) => {
		let marked: MarkedInputEvidence | null = null;
		await journeyGridSizes.reduce(async (previous, dimensions) => {
			await previous;
			requireChild(context);
			const [columns, rows] = dimensions.split('x').map(Number);
			if (columns === undefined || rows === undefined)
				throw new Error('quota resize dimensions absent');
			const prior = marked === null ? context.frame : quotaMarkedFrame(marked);
			marked =
				prior.grid.columns === columns && prior.grid.rows === rows
					? await context.input.press('F12', [quotaExpectedKey('F12', quotaScreen)])
					: await context.input.resize(columns, rows, [
							{
								event: { kind: 'resize', columns, rows },
								surface: quotaScreen,
								target: null,
								action: null,
							},
						]);
			const failures = checkQuotaToolbar(quotaMarkedFrame(marked).grid);
			if (failures.length > 0) throw new Error(failures.join('; '));
			await Array.from({ length: 12 }).reduce(async (previousHop) => {
				await previousHop;
				marked = await context.input.press('Tab', [quotaExpectedKey('Tab', quotaScreen)]);
				const grid = quotaMarkedFrame(marked).grid,
					{ row } = quotaToolbarRow(grid);
				if (
					!quotaListFocused(grid) &&
					!(grid.cells[row] ?? []).some(
						(cell) => cell.fgRgb === terminalTheme.palette[11] && (cell.flags & 0x08) !== 0,
					)
				)
					throw new Error('quota Tab focused an unpainted control after resize');
			}, Promise.resolve());
			if (marked === null) throw new Error('quota resize has no marked frame');
			const frame = quotaMarkedFrame(marked);
			await renderOriginal({
				...original,
				output: evidence,
				checkpoint: 'toolbar-resize-' + dimensions,
				columns,
				rows,
				bytes: frame.bytes,
				grid: frame.grid,
				resizes: frame.resizes,
			});
			requireChild(context);
		}, Promise.resolve());
		resizeQualified = true;
		return marked;
	});
	const menuFailures = (): readonly string[] =>
		menu === null ? ['quota menu was not traversed'] : [];
	return {
		target: {
			repository: plan.observerRepository,
			driver: plan.driver.path,
			node: plan.node,
			surface: 'normal',
			componentBinary: plan.product.path,
			componentScenario: 'normal',
			componentDispatchReceipt: join(evidence, 'dispatch.jsonl'),
			keys: invocation.keys,
			evidence,
		},
		receipt: join(evidence, 'dispatch.jsonl'),
		original,
		ready: (grid) => grid.cells.some((row) => row.some((cell) => cellText(cell) === '≡')),
		expectedKeyboardFlags: invocation.keys === 'legacy' ? 0 : 2,
		initialBarrier: { key: 'F12', expected: [quotaExpectedKey('F12', quotaScreen)] },
		resizeExpected: (columns, rows) => [
			{
				event: { kind: 'resize', columns, rows },
				surface: quotaScreen,
				target: null,
				action: null,
			},
		],
		inputs,
		fixtures,
		readbacks: new Map(),
		targets: new Map([
			[
				'≡ Menu',
				(grid) => ({ label: 'Menu', bounds: { left: 0, right: grid.columns, top: 1, bottom: 2 } }),
			],
			[
				'main list',
				(grid) => ({
					label: '東',
					bounds: { left: 0, right: grid.columns, top: 2, bottom: grid.rows },
				}),
			],
		]),
		checks: new Map([
			[
				'quota-toolbar',
				async (context: NativeFlowContext) => {
					requireChild(context);
					return checkQuotaToolbar(context.frame.grid);
				},
			],
			[
				'no-bottom-actions',
				async (context: NativeFlowContext) => checkNoBottomActions(context.frame.grid),
			],
			['closed-menu-inventory', async () => menuFailures()],
			['menu-key-and-tab', async () => menuFailures()],
			[
				'projection-parity',
				async (context: NativeFlowContext) => [
					...menuFailures(),
					...(findDialog(context.frame.grid, 'Help') !== null && !helpQualified
						? ['quota Help census absent']
						: []),
				],
			],
			[
				'right-dropoff',
				async (context: NativeFlowContext) => {
					requireChild(context);
					return resizeQualified ? [] : ['same-child quota resize proof absent'];
				},
			],
		]),
		quit: {
			key: 'Ctrl+C',
			expected: [quotaExpectedKey('Ctrl+C', quotaScreen, null, 'quota:quit')],
		},
	};
}
