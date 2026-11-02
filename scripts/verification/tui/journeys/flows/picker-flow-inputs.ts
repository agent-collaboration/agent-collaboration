/** Source-derived picker input ownership and shared native fixture transport. */
import { join } from 'node:path';

import type { InputConsumptionExpectation } from '../../sessions/input-consumption-schema.ts';
import type { MarkedInputEvidence } from '../../terminal/marker-input-session.ts';
import type { PtyGridTarget } from '../../terminal/pty-grid-driver.ts';
import { cellText, terminalTheme } from '../../terminal/terminal-grid.ts';
import type { TerminalGrid } from '../../terminal/terminal-grid.ts';
import { findDialog } from '../scenes/grid-scene-matcher.ts';
import type { AuthoredFlowPlan } from './authored-flow-plan.ts';
import type { FlowInvocation } from './flow-executor.ts';
import type { NativeFlowBindings, NativeFlowContext } from './native-flow-runtime.ts';
import { locatePaintedTarget } from './painted-flow-target.ts';
import type { PaintedFlowTarget } from './painted-flow-target.ts';

export const pickerScreen = { kind: 'screen' } as const;
/** crossterm adds Shift for raw uppercase; CSI-u text carries its own modifiers. */
export function pickerFilterTextExpectations(
	text: string,
	kittyFlags: number,
): readonly InputConsumptionExpectation[] {
	return Array.from(text, (character) => ({
		event: {
			kind: 'key',
			code: { kind: 'text' },
			modifiers: (kittyFlags & 8) === 0 && /^[A-Z]$/u.test(character) ? ['shift'] : [],
			phase: 'press',
		},
		surface: pickerScreen,
		target: 'session-filter',
		action: null,
	}));
}
export function pickerOverlay(ordinal: number): InputConsumptionExpectation['surface'] {
	return { kind: 'overlay', overlayId: String(ordinal) };
}
/** An unchanged kernel size need not produce SIGWINCH; use an explicit harmless input instead. */
export function resizePickerWhenChanged(options: {
	readonly context: NativeFlowContext;
	readonly columns: number;
	readonly rows: number;
	readonly surface: InputConsumptionExpectation['surface'];
}): Promise<MarkedInputEvidence> {
	const { context, columns, rows, surface } = options;
	if (context.frame.grid.columns === columns && context.frame.grid.rows === rows)
		return context.input.press('F12', [pickerExpectedKey('F12', surface)]);
	return context.input.resize(columns, rows, [
		{ event: { kind: 'resize', columns, rows }, surface, target: null, action: null },
	]);
}
export function pickerExpectedKey(
	key: string,
	surface: InputConsumptionExpectation['surface'],
	target: string | null = null,
	action: string | null = null,
): InputConsumptionExpectation {
	const parts = key.split('+');
	const name = parts.at(-1);
	if (name === undefined) throw new Error('empty picker key');
	const modifiers: ('shift' | 'control' | 'alt')[] = [];
	if (parts.includes('Shift')) modifiers.push('shift');
	if (parts.includes('Ctrl')) modifiers.push('control');
	if (parts.includes('Alt')) modifiers.push('alt');
	const shortcut = parts.includes('Ctrl') && name.length === 1;
	return {
		event: {
			kind: 'key',
			code: shortcut
				? { kind: 'shortcut', key: name.toLowerCase() }
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
export function pickerDialogTarget(
	grid: TerminalGrid,
	title: string,
	label: string,
): PaintedFlowTarget {
	const rectangle = findDialog(grid, title);
	if (rectangle === null) throw new Error('active picker dialog absent: ' + title);
	return {
		label,
		bounds: {
			left: rectangle.left + 1,
			top: rectangle.top + 1,
			right: rectangle.right,
			bottom: rectangle.bottom,
		},
	};
}
export function pickerFocused(grid: TerminalGrid, title: string, label: string): boolean {
	try {
		const location = locatePaintedTarget(grid, {
			...pickerDialogTarget(grid, title, label),
			foregrounds: [terminalTheme.palette[11] ?? 0xd7ba7d],
		});
		return (
			grid.cells[location.row]
				?.slice(location.column, location.column + location.width)
				.every((cell) => cell.fgRgb === terminalTheme.palette[11]) === true
		);
	} catch {
		return false;
	}
}
export function pickerPrimaryTarget(
	grid: TerminalGrid,
	title: string,
	label: string,
): PaintedFlowTarget {
	return {
		...pickerDialogTarget(grid, title, label),
		foregrounds: [terminalTheme.palette[11] ?? 0xd7ba7d, 0x9cbcfb],
	};
}
export function pickerDialogText(grid: TerminalGrid, title: string): string {
	const rectangle = findDialog(grid, title);
	if (rectangle === null) throw new Error('active picker dialog absent: ' + title);
	return grid.cells
		.slice(rectangle.top + 1, rectangle.bottom)
		.map((row) =>
			row
				.slice(rectangle.left + 1, rectangle.right)
				.map(cellText)
				.join(''),
		)
		.join('\n');
}
export function pickerBaseBindings(options: {
	readonly plan: AuthoredFlowPlan;
	readonly invocation: FlowInvocation;
	readonly evidence: string;
	readonly receipts: string;
	readonly variant: NonNullable<PtyGridTarget['storyboardVariant']>;
}): NativeFlowBindings {
	const { plan, invocation, evidence, receipts, variant } = options;
	return {
		target: {
			repository: plan.observerRepository,
			driver: plan.driver.path,
			product: plan.product,
			node: plan.node,
			surface: 'sessions-filters',
			keys: invocation.keys,
			evidence,
			sessionsReceiptDirectory: receipts,
			storyboardVariant: variant,
		},
		receipt: join(receipts, 'input-dispatch.jsonl'),
		original: {
			chrome: plan.chrome,
			sourceHead: plan.productSource.head,
			binarySha256: plan.product.sha256,
		},
		ready: (grid) => grid.cells.some((row) => row.some((cell) => cellText(cell) === '≡')),
		expectedKeyboardFlags: invocation.keys === 'legacy' ? 0 : 2,
		initialBarrier: { key: 'F12', expected: [pickerExpectedKey('F12', pickerScreen)] },
		resizeExpected: (columns, rows) => [
			{
				event: { kind: 'resize', columns, rows },
				surface: pickerScreen,
				target: null,
				action: null,
			},
		],
		inputs: new Map(),
		targets: new Map(),
		fixtures: new Map(),
		readbacks: new Map(),
		checks: new Map(),
		quit: { key: 'Ctrl+C', expected: [pickerExpectedKey('Ctrl+C', pickerScreen, null, 'quit')] },
	};
}
