/** R13 native toolbar, menu, Help, New Run choices and filter-focus observations. */
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
import { terminalTheme } from '../../terminal/terminal-grid.ts';
import type { TerminalGrid } from '../../terminal/terminal-grid.ts';
import { renderOriginal } from '../../terminal/terminal-original.ts';
import { journeyGridSizes } from '../journey-definition.ts';
import { findDialog, gridFieldPair } from '../scenes/grid-scene-matcher.ts';
import { checkNoBottomActions } from '../scenes/public-entry-scene-checks.ts';
import type { AuthoredFlowPlan } from './authored-flow-plan.ts';
import type { FlowInvocation } from './flow-executor.ts';
import type {
	NativeFlowBindings,
	NativeFlowContext,
	NativeFlowInputBinding,
} from './native-flow-runtime.ts';
import type { PaintedFlowTarget, PaintedTargetLocation } from './painted-flow-target.ts';
import { locatePaintedTarget } from './painted-flow-target.ts';
import {
	pickerBaseBindings,
	pickerDialogTarget,
	pickerExpectedKey,
	pickerFilterTextExpectations,
	pickerFocused,
	pickerOverlay,
	pickerScreen,
	resizePickerWhenChanged,
} from './picker-flow-inputs.ts';
import { pickerMarkedFrame, qualifyPickerHelp } from './picker-help-observation.ts';
import { collectPickerMenu } from './picker-menu-observation.ts';
import type { PickerMenuObservation } from './picker-menu-observation.ts';
import { checkNewKeyHints } from './picker-new-bindings.ts';
import {
	checkCurrentPickerToolbar,
	pickerFilterFocused,
	pickerListFocused,
	pickerToolbarLabelFocused,
	pickerToolbarRow,
	pickerVisibleFocus,
} from './picker-toolbar-observation.ts';

function mouseExpectation(
	location: PaintedTargetLocation,
	phase: 'down' | 'up',
	surface: InputConsumptionExpectation['surface'],
	target: string | null,
	action: string | null,
): InputConsumptionExpectation {
	return {
		event: {
			kind: 'mouse',
			mouse: { kind: phase, button: 'left' },
			column: location.column,
			row: location.row,
			modifiers: [],
		},
		surface,
		target,
		action,
	};
}
function menuChoiceFocused(grid: TerminalGrid, title: string): boolean {
	try {
		const bounds = findDialog(grid, '');
		if (bounds === null) return false;
		locatePaintedTarget(grid, {
			label: title,
			bounds,
			foregrounds: [terminalTheme.palette[11] ?? 0xd7ba7d],
		});
		return true;
	} catch {
		return false;
	}
}

export function pickerToolbarBindings(
	plan: AuthoredFlowPlan,
	invocation: FlowInvocation,
	evidence: string,
	receipts: string,
): NativeFlowBindings | null {
	if (invocation.journey.journeyId !== 'sessions-toolbar-and-menu') return null;
	const base = pickerBaseBindings({
		plan,
		invocation,
		evidence,
		receipts,
		variant: 'claude-new-success',
	});
	const inputs = new Map<number, NativeFlowInputBinding>();
	let active: InputConsumptionExpectation['surface'] = pickerScreen;
	let nextOverlay = 1;
	for (const [index, step] of invocation.journey.steps.entries()) {
		if (step.kind === 'press') {
			let expected: InputConsumptionExpectation;
			if (step.key === 'F10' || step.key === 'Enter') {
				expected = pickerExpectedKey(
					step.key,
					pickerScreen,
					step.key === 'Enter' ? 'action:menu' : null,
					'menu',
				);
				active = pickerOverlay(nextOverlay++);
			} else if (step.key === 'F1') {
				expected = pickerExpectedKey('F1', pickerScreen, null, 'help');
				active = pickerOverlay(nextOverlay++);
			} else if (step.key === '/')
				expected = {
					event: { kind: 'key', code: { kind: 'text' }, modifiers: [], phase: 'press' },
					surface: pickerScreen,
					target: null,
					action: 'focus-filter',
				};
			else if (step.key === 'Escape') {
				expected = pickerExpectedKey(
					'Escape',
					active,
					active.kind === 'screen'
						? 'session-filter'
						: active.overlayId === '3'
							? '11:action:help:help-body'
							: null,
				);
				active = pickerScreen;
			} else throw new Error('toolbar key not source-declared: ' + step.key);
			inputs.set(index, { kind: 'continuing', expected: [expected] });
		} else if (step.kind === 'focus') {
			const run = step.target === 'Run';
			const key = step.via === 'Shift+Tab' ? 'Shift+Tab' : 'Tab';
			inputs.set(index, {
				kind: 'focus',
				stopWhenFocused: true,
				route: Array.from({ length: 12 }, () => ({
					key,
					expected: [pickerExpectedKey(key, run ? pickerOverlay(6) : pickerScreen)],
				})),
				isFocused: (grid) =>
					run
						? pickerFocused(grid, 'New session', 'Background')
						: step.target === 'main list'
							? pickerListFocused(grid)
							: pickerToolbarLabelFocused(grid, 'Menu'),
			});
		} else if (step.kind === 'type')
			inputs.set(index, {
				kind: 'continuing',
				expected: pickerFilterTextExpectations(step.text, invocation.keys === 'kitty' ? 2 : 0),
			});
		else if (step.kind === 'click') {
			const before = active;
			let downTarget: string | null = null,
				downAction: string | null = null;
			if (step.target === '≡ Menu') {
				downTarget = 'action:menu';
				downAction = 'menu';
				active = pickerOverlay(nextOverlay++);
			} else if (step.target === 'New background session…') {
				downTarget = 'action:new-background';
				downAction = 'new-background';
				active = pickerOverlay(nextOverlay++);
			} else if (step.target === 'Run') {
				downTarget = 'new-run';
				downAction = 'new-run';
				active = pickerOverlay(nextOverlay++);
			} else if (step.target === 'Foreground') {
				downTarget = '7:new-run:option:run:ClaudeNativeForeground';
				active = pickerOverlay(6);
			} else if (step.target === 'Filter') downTarget = 'session-filter';
			else throw new Error('toolbar click not source-declared');
			const after = active;
			inputs.set(index, {
				kind: 'click',
				ending: 'continue',
				expected: (location) => [
					mouseExpectation(location, 'down', before, downTarget, downAction),
					mouseExpectation(location, 'up', after, null, null),
				],
			});
		}
	}
	let child: ProcessIdentity | null = null;
	let menu: PickerMenuObservation | null = null;
	let helpQualified = false;
	let runCheck = 0;
	let filterCheck = 0;
	let resizeQualified = false;
	const requireChild = (context: NativeFlowContext): void => {
		child ??= requireOwnedProductChild(context.driver.processId(), plan.product.path);
		if (!sameIdentity(lookupProcessIdentity(child.pid), child))
			throw new Error('toolbar observation lost the exact product identity');
	};
	const menuFailures = (): readonly string[] =>
		menu === null ? ['real menu has not been traversed'] : menu.failures;
	const fixtures = new Map<
		string,
		(context: NativeFlowContext) => Promise<MarkedInputEvidence | null>
	>();
	for (const [checkpoint, ordinal] of [
		['menu-f10', 1],
		['menu-from-click', 2],
		['menu-from-tab', 4],
		['narrow-menu-complete', 8],
	] as const)
		fixtures.set('inspect-picker-menu-' + checkpoint, async (context) => {
			const collected = await collectPickerMenu({ context, surface: pickerOverlay(ordinal) });
			menu = collected.observation;
			writeFileSync(join(evidence, checkpoint + '-inventory.json'), JSON.stringify(menu, null, 2));
			return collected.marked;
		});
	fixtures.set('inspect-picker-menu-help', async (context) => {
		const qualified = await qualifyPickerHelp({ context, surface: pickerOverlay(3) });
		helpQualified = true;
		writeFileSync(
			join(evidence, 'toolbar-help-census.json'),
			JSON.stringify(qualified.qualified, null, 2),
		);
		return qualified.marked;
	});
	fixtures.set('new-client-native-claude-with-both-run-modes', async () => null);
	fixtures.set('reveal-picker-background-new', async (context) => {
		let marked = await context.input.press('F12', [pickerExpectedKey('F12', pickerOverlay(5))]);
		const reveal = async (remaining: number): Promise<void> => {
			if (menuChoiceFocused(pickerMarkedFrame(marked).grid, 'New background session…')) return;
			if (remaining === 0) throw new Error('New background menu action was not revealed');
			marked = await context.input.press('Down', [pickerExpectedKey('Down', pickerOverlay(5))]);
			await reveal(remaining - 1);
		};
		await reveal(32);
		return marked;
	});
	fixtures.set('resize-same-child-to-each-declared-grid', async (context) => {
		let marked: MarkedInputEvidence | null = null;
		await journeyGridSizes.reduce(async (previous, dimensions) => {
			await previous;
			requireChild(context);
			const [columns, rows] = dimensions.split('x').map(Number);
			if (columns === undefined || rows === undefined)
				throw new Error('toolbar dimensions missing');
			marked = await resizePickerWhenChanged({
				context: { ...context, frame: marked === null ? context.frame : pickerMarkedFrame(marked) },
				columns,
				rows,
				surface: pickerScreen,
			});
			const failures = checkCurrentPickerToolbar(pickerMarkedFrame(marked).grid);
			if (failures.length > 0) throw new Error(failures.join('; '));
			await Array.from({ length: 12 }).reduce(async (prior) => {
				await prior;
				marked = await context.input.press('Tab', [pickerExpectedKey('Tab', pickerScreen)]);
				if (!pickerVisibleFocus(pickerMarkedFrame(marked).grid))
					throw new Error('Tab focused a control that is not painted after dropoff');
			}, Promise.resolve());
			if (marked === null) throw new Error('toolbar resize has no marked frame');
			const frame = pickerMarkedFrame(marked);
			await renderOriginal({
				...base.original,
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
	const toolbar = async (context: NativeFlowContext): Promise<readonly string[]> => {
		requireChild(context);
		return checkCurrentPickerToolbar(context.frame.grid);
	};
	return {
		...base,
		inputs,
		fixtures,
		targets: new Map<string, (grid: TerminalGrid) => PaintedFlowTarget>([
			[
				'≡ Menu',
				(grid) => ({ label: 'Menu', bounds: { left: 0, right: grid.columns, top: 1, bottom: 2 } }),
			],
			[
				'main list',
				(grid) => {
					const bounds = findDialog(grid, 'Session');
					if (bounds === null) throw new Error('main list not painted');
					return { label: 'Session', bounds };
				},
			],
			[
				'New background session…',
				(grid) => {
					const bounds = findDialog(grid, '');
					if (bounds === null) throw new Error('New background menu is not painted');
					return {
						label: 'New background session…',
						bounds,
						foregrounds: [terminalTheme.palette[11] ?? 0xd7ba7d],
					};
				},
			],
			['Run', (grid) => pickerDialogTarget(grid, 'New session', 'Background')],
			['Foreground', (grid) => pickerDialogTarget(grid, 'Filter:', 'Foreground')],
			[
				'Filter',
				(grid) => ({
					label: 'Filter…',
					bounds: { left: 0, right: grid.columns, top: 1, bottom: 2 },
				}),
			],
		]),
		checkpointBarriers: new Map([
			[
				'new-background-preset',
				{
					ready: (grid) =>
						gridFieldPair(grid, 'New session', 'Run', 'Background') &&
						gridFieldPair(grid, 'New session', 'Client', 'Claude Code'),
					publish: { key: 'F12', expected: [pickerExpectedKey('F12', pickerOverlay(6))] },
				},
			],
		]),
		checks: new Map([
			['picker-toolbar', toolbar],
			[
				'right-dropoff',
				async (context) => {
					requireChild(context);
					return resizeQualified ? [] : ['same-child toolbar/dropoff resize proof absent'];
				},
			],
			['no-bottom-actions', async (context) => checkNoBottomActions(context.frame.grid)],
			['closed-menu-inventory', async () => menuFailures()],
			['menu-key-and-tab', async () => menuFailures()],
			[
				'checked-settings',
				async () => {
					const checked = menu?.items.filter((item) => item.checked).map((item) => item.title);
					return JSON.stringify(checked) ===
						JSON.stringify(['All', 'Runtime: All', 'Sort: Updated', 'Group by: None'])
						? []
						: ['menu checked settings differ from the declared fixture'];
				},
			],
			[
				'projection-parity',
				async (context) => {
					if (findDialog(context.frame.grid, 'Help') !== null && !helpQualified)
						return ['full Help census was not observed'];
					return menuFailures();
				},
			],
			[
				'help-conventions-and-menu-control',
				async () => (helpQualified ? [] : ['Help action/settings/convention census absent']),
			],
			[
				'new-run-mode-by-capability',
				async (context) => {
					const expected = runCheck++ === 0 ? 'Background' : 'Foreground';
					return [
						...checkNewKeyHints(context.frame.grid),
						...(gridFieldPair(context.frame.grid, 'New session', 'Run', expected) &&
						gridFieldPair(context.frame.grid, 'New session', 'Client', 'Claude Code')
							? []
							: ['Claude Run choice differs from its independently declared mode']),
					];
				},
			],
			[
				'filter-focus-and-live-query',
				async (context) => {
					const live = filterCheck++ === 2;
					if (
						findDialog(context.frame.grid, 'Session details') !== null ||
						!pickerFilterFocused(context.frame.grid)
					)
						return ['filter click/shortcut failed to focus the field or opened Details'];
					if (
						live &&
						!pickerToolbarRow(context.frame.grid).text.includes(
							context.frame.grid.columns < 40 ? '3456789A' : '0123456789A',
						)
					)
						return ['live filter query is absent from its visible editor'];
					return [];
				},
			],
		]),
	};
}
