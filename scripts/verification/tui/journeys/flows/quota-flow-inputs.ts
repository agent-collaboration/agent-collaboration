/** Source-bound quota ACKs and painted selectors shared by the native quota journeys. */
import { join } from 'node:path';

import type { ComponentScenario } from '../../fixtures/component-fixture-contract.ts';
import type { InputConsumptionExpectation } from '../../sessions/input-consumption-schema.ts';
import type { MarkedInputEvidence } from '../../terminal/marker-input-session.ts';
import { cellText, gridLines, terminalTheme } from '../../terminal/terminal-grid.ts';
import type { TerminalGrid } from '../../terminal/terminal-grid.ts';
import { findDialog } from '../scenes/grid-scene-matcher.ts';
import { trackedScrollViewport } from '../scenes/option-navigation-observation.ts';
import type { AuthoredFlowPlan } from './authored-flow-plan.ts';
import type { FlowInvocation } from './flow-executor.ts';
import type { NativeFlowBindings, NativeFlowContext } from './native-flow-runtime.ts';
import type { PaintedFlowTarget, PaintedTargetLocation } from './painted-flow-target.ts';
import { locatePaintedTarget } from './painted-flow-target.ts';
import type { QuotaFixtureLayout } from './quota-fixture-layout.ts';

export const quotaScreen = { kind: 'screen' } as const;
export function quotaOverlay(ordinal: number): InputConsumptionExpectation['surface'] {
	return { kind: 'overlay', overlayId: String(ordinal) };
}
export function quotaExpectedKey(
	key: string,
	surface: InputConsumptionExpectation['surface'],
	target: string | null = null,
	action: string | null = null,
): InputConsumptionExpectation {
	const parts = key.split('+'),
		name = parts.at(-1);
	if (name === undefined) throw new Error('quota key absent');
	const modifiers: ('shift' | 'control' | 'alt')[] = [];
	if (parts.includes('Shift') || (parts.length === 1 && /^[A-Z]$/u.test(name)))
		modifiers.push('shift');
	if (parts.includes('Ctrl')) modifiers.push('control');
	if (parts.includes('Alt')) modifiers.push('alt');
	return {
		event: {
			kind: 'key',
			code:
				parts.includes('Ctrl') && name.length === 1
					? { kind: 'shortcut', key: name.toLowerCase() }
					: name.length === 1
						? { kind: 'text' }
						: {
								kind: 'fixed',
								name:
									name === 'Tab' && parts.includes('Shift')
										? 'Back Tab'
										: name === 'Enter' && process.platform === 'darwin'
											? 'Return'
											: name === 'Escape'
												? 'Esc'
												: name,
							},
			modifiers,
			phase: 'press',
		},
		surface,
		target,
		action,
	};
}
export function quotaMouse(
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
export function quotaMarkedFrame(marked: MarkedInputEvidence): NativeFlowContext['frame'] {
	const frame = marked.frames.at(-1);
	if (frame === undefined) throw new Error('quota input has no marked frame');
	return frame;
}
export function quotaDialogTarget(
	grid: TerminalGrid,
	title: string,
	label: string,
): PaintedFlowTarget {
	const rectangle = findDialog(grid, title);
	if (rectangle === null) throw new Error('active quota dialog absent: ' + title);
	return {
		label,
		bounds: {
			left: rectangle.left + 1,
			top: rectangle.top + 1,
			right: rectangle.right,
			bottom: rectangle.bottom,
		},
		foregrounds: [
			terminalTheme.palette[11] ?? 0xd7ba7d,
			0x9cbcfb,
			terminalTheme.palette[7] ?? 0xd4d4d4,
			terminalTheme.palette[15] ?? 0xffffff,
		],
	};
}
export function quotaFocused(grid: TerminalGrid, title: string, label: string): boolean {
	try {
		locatePaintedTarget(grid, {
			...quotaDialogTarget(grid, title, label),
			foregrounds: [terminalTheme.palette[11] ?? 0xd7ba7d],
		});
		return true;
	} catch {
		return false;
	}
}
export function quotaPrimary(grid: TerminalGrid, title: string, label: string): boolean {
	const rectangle = findDialog(grid, title);
	if (rectangle === null) return false;
	return gridLines(grid)
		.slice(rectangle.top + 1, rectangle.bottom)
		.filter((line) => line.includes('●'))
		.some((line) => line.includes('● ' + label));
}
export function quotaBaseBindings(options: {
	readonly plan: AuthoredFlowPlan;
	readonly invocation: FlowInvocation;
	readonly evidence: string;
	readonly layout: QuotaFixtureLayout;
}): NativeFlowBindings {
	const { plan, invocation, evidence, layout } = options;
	return {
		target: {
			repository: plan.observerRepository,
			driver: plan.driver.path,
			product: plan.product,
			node: plan.node,
			surface: 'quota-effects',
			keys: invocation.keys,
			evidence,
			quotaEffectEnvironment: layout.environment,
		},
		receipt: layout.configuration.dispatchReceipt,
		original: {
			chrome: plan.chrome,
			sourceHead: plan.productSource.head,
			binarySha256: plan.product.sha256,
		},
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
		inputs: new Map(),
		targets: new Map(),
		fixtures: new Map(),
		readbacks: new Map(),
		checks: new Map(),
		quit: {
			key: 'Ctrl+C',
			expected: [quotaExpectedKey('Ctrl+C', quotaScreen, null, 'quota:quit')],
		},
	};
}

/** Acknowledge each physical body row, retain the complete text, and restore the entry scroll. */
export async function collectQuotaDialog(options: {
	readonly context: NativeFlowContext;
	readonly title: string;
	readonly surface: InputConsumptionExpectation['surface'];
	readonly focusedTarget: string;
}): Promise<{ readonly marked: MarkedInputEvidence; readonly rows: readonly string[] }> {
	const { context, title, surface, focusedTarget } = options;
	let marked = await context.input.press('F12', [quotaExpectedKey('F12', surface)]);
	const rectangle = findDialog(quotaMarkedFrame(marked).grid, title);
	if (rectangle === null) throw new Error('quota body absent: ' + title);
	let viewport = trackedScrollViewport(quotaMarkedFrame(marked).grid, rectangle);
	if (viewport === null)
		return {
			marked,
			rows: quotaDialogInteriorRows(quotaMarkedFrame(marked).grid, title),
		};
	const full = [...viewport.rows];
	let moved = 0;
	const advance = async (): Promise<void> => {
		if (viewport === null) throw new Error('quota body track vanished');
		if (viewport.atEnd) return;
		if (moved >= 256) throw new Error('quota body exceeds the bounded physical-row census');
		marked = await context.input.press('Down', [quotaExpectedKey('Down', surface, focusedTarget)]);
		const next = trackedScrollViewport(quotaMarkedFrame(marked).grid, rectangle);
		if (
			next === null ||
			next.rows.length !== viewport.rows.length ||
			viewport.rows.slice(1).some((row, index) => row !== next.rows[index])
		)
			throw new Error('quota Down did not advance the physical body exactly one row');
		const last = next.rows.at(-1);
		if (last === undefined) throw new Error('quota body empty');
		full.push(last);
		viewport = next;
		moved += 1;
		await advance();
	};
	await advance();
	await Array.from({ length: moved }).reduce(async (previous) => {
		await previous;
		marked = await context.input.press('Up', [quotaExpectedKey('Up', surface, focusedTarget)]);
	}, Promise.resolve());
	const restored = trackedScrollViewport(quotaMarkedFrame(marked).grid, rectangle);
	if (
		restored === null ||
		JSON.stringify(restored.rows) !== JSON.stringify(full.slice(0, restored.rows.length))
	)
		throw new Error('quota body census did not restore entry scroll');
	return { marked, rows: full };
}

/** The read-only libtest is nominated through the existing component route, independently of the CLI role. */
export function quotaReadOnlyBaseBindings(
	plan: AuthoredFlowPlan,
	invocation: FlowInvocation,
	evidence: string,
	scenario: ComponentScenario,
): NativeFlowBindings {
	return {
		target: {
			repository: plan.observerRepository,
			driver: plan.driver.path,
			node: plan.node,
			surface: 'normal',
			componentBinary: plan.product.path,
			componentScenario: scenario,
			componentDispatchReceipt: join(evidence, 'dispatch.jsonl'),
			keys: invocation.keys,
			evidence,
		},
		receipt: join(evidence, 'dispatch.jsonl'),
		original: {
			chrome: plan.chrome,
			sourceHead: plan.productSource.head,
			binarySha256: plan.product.sha256,
		},
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
		inputs: new Map(),
		targets: new Map(),
		fixtures: new Map(),
		readbacks: new Map(),
		checks: new Map(),
		quit: {
			key: 'Ctrl+C',
			expected: [quotaExpectedKey('Ctrl+C', quotaScreen, null, 'quota:quit')],
		},
	};
}

/** Physical rows for a fitting dialog; overflow uses the actual tracked content interval. */
export function quotaDialogInteriorRows(grid: TerminalGrid, title: string): readonly string[] {
	const rectangle = findDialog(grid, title);
	if (rectangle === null) throw new Error('quota dialog absent: ' + title);
	return grid.cells.slice(rectangle.top + 1, rectangle.bottom).map((row) =>
		row
			.slice(rectangle.left + 1, rectangle.right)
			.map(cellText)
			.join('')
			.trimEnd(),
	);
}
