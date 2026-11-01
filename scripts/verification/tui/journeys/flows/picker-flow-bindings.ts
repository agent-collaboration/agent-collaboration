/** Fixture adapter for the stopped-native-Claude flow; other fixtures remain admission gaps. */
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

import type { InputConsumptionExpectation } from '../../sessions/input-consumption-schema.ts';
import {
	checkSourceQualifiedOutcome,
	parsePickerOutcomeReadback,
} from '../../sessions/source-qualified-outcome.ts';
import { terminalTheme } from '../../terminal/terminal-grid.ts';
import type { TerminalGrid } from '../../terminal/terminal-grid.ts';
import { findDialog } from '../scenes/grid-scene-matcher.ts';
import { checkStoppedClaudeDetails } from '../scenes/public-entry-scene-checks.ts';
import type { AuthoredFlowPlan } from './authored-flow-plan.ts';
import type { FlowInvocation } from './flow-executor.ts';
import type { NativeFlowBindings } from './native-flow-runtime.ts';
import { locatePaintedTarget } from './painted-flow-target.ts';
import type { PaintedFlowTarget } from './painted-flow-target.ts';
import { pickerFilterTextExpectations } from './picker-flow-inputs.ts';
import { pickerIdentityBindings } from './picker-identity-bindings.ts';
import { pickerNewBindings } from './picker-new-bindings.ts';

const screen = { kind: 'screen' } as const;
// This journey creates exactly one overlay. SurfaceStack::new starts at 1;
// no expected route is learned from the ACK under test.
const details = { kind: 'overlay', overlayId: '1' } as const;
function fixed(
	name: string,
	surface: InputConsumptionExpectation['surface'],
	target: string | null = null,
	action: string | null = null,
): InputConsumptionExpectation {
	return {
		// Wire fixed names use crossterm Display, whose macOS Enter is Return.
		event: {
			kind: 'key',
			code: {
				kind: 'fixed',
				name: name === 'Enter' && process.platform === 'darwin' ? 'Return' : name,
			},
			modifiers: [],
			phase: 'press',
		},
		surface,
		target,
		action,
	};
}
export const stoppedClaudeExpectedOutcome = {
	kind: 'resume',
	identity: {
		kind: 'nativeClaude',
		target: {
			endpoint: { serviceId: '00000000-0000-4000-8000-000000000001', endpointId: 'claude-local' },
			sessionId: '00000000-0000-4000-8000-000000000011',
		},
	},
} as const;
export function stoppedClaudeOpenTarget(grid: TerminalGrid): PaintedFlowTarget {
	const rect = findDialog(grid, 'Session details');
	if (rect === null) throw new Error('stopped Claude Details surface absent');
	return {
		label: 'Open',
		bounds: { left: rect.left + 1, top: rect.top + 1, right: rect.right, bottom: rect.bottom },
		foregrounds: [terminalTheme.palette[11] ?? 0xd7ba7d, 0x9cbcfb],
	};
}
function openFocused(grid: TerminalGrid): boolean {
	try {
		const location = locatePaintedTarget(grid, stoppedClaudeOpenTarget(grid));
		return (
			grid.cells[location.row]
				?.slice(location.column, location.column + location.width)
				.every((cell) => cell.fgRgb === terminalTheme.palette[11]) === true
		);
	} catch {
		return false;
	}
}
export function pickerFlowBindings(
	plan: AuthoredFlowPlan,
	invocation: FlowInvocation,
	evidence: string,
	receiptDirectory: string,
): NativeFlowBindings | null {
	if (invocation.journey.journeyId !== 'stopped-claude-details-open-only')
		return (
			pickerNewBindings(plan, invocation, evidence, receiptDirectory) ??
			pickerIdentityBindings(plan, invocation, evidence, receiptDirectory)
		);
	const ignored = fixed('F12', details);
	return {
		target: {
			repository: plan.observerRepository,
			driver: plan.driver.path,
			product: plan.product,
			node: plan.node,
			surface: 'sessions-filters',
			keys: invocation.keys,
			evidence,
			sessionsReceiptDirectory: receiptDirectory,
			storyboardVariant: 'stopped-native-claude',
		},
		receipt: join(receiptDirectory, 'input-dispatch.jsonl'),
		original: {
			chrome: plan.chrome,
			sourceHead: plan.productSource.head,
			binarySha256: plan.product.sha256,
		},
		ready: (grid) =>
			grid.cells.some((row) => row.some((cell) => cell.chars === '≡' || cell.char === 0x2261)),
		expectedKeyboardFlags: invocation.keys === 'legacy' ? 0 : 2,
		initialBarrier: { key: 'F12', expected: [fixed('F12', screen)] },
		resizeExpected: (columns, rows) => [
			{ event: { kind: 'resize', columns, rows }, surface: screen, target: null, action: null },
		],
		checkpointBarriers: new Map([
			[
				'stopped-claude-details',
				{
					ready: (grid) => checkStoppedClaudeDetails(grid).length === 0,
					publish: { key: 'F12', expected: [ignored] },
				},
			],
		]),
		inputs: new Map([
			[1, { kind: 'continuing', expected: [fixed('Enter', screen, 'session-filter', 'details')] }],
			[
				3,
				{
					kind: 'focus',
					route: Array.from({ length: 4 }, () => ({
						key: 'Tab',
						expected: [fixed('Tab', details)],
					})),
					stopWhenFocused: true,
					isFocused: openFocused,
				},
			],
			[
				4,
				{
					kind: 'exit',
					expected: [fixed('Enter', details, 'action:details-open', 'details-open')],
				},
			],
		]),
		targets: new Map([['Open', stoppedClaudeOpenTarget]]),
		fixtures: new Map([
			[
				'select-stopped-native-claude',
				async (context) => {
					await context.input.press('/', [
						{
							event: { kind: 'key', code: { kind: 'text' }, modifiers: [], phase: 'press' },
							surface: screen,
							target: null,
							action: 'focus-filter',
						},
					]);
					return context.input.text(
						'Stopped native Claude',
						pickerFilterTextExpectations(
							'Stopped native Claude',
							context.driver.inputModes().kittyFlags,
						),
					);
				},
			],
		]),
		readbacks: new Map([
			[
				'source-qualified-outcome',
				async () =>
					parsePickerOutcomeReadback(readFileSync(join(receiptDirectory, 'picker-outcome.json'))),
			],
		]),
		checks: new Map([
			[
				'stopped-claude-open-only',
				async (context) => checkStoppedClaudeDetails(context.frame.grid),
			],
			[
				'captured-session-outcome',
				async (_context, value): Promise<readonly string[]> => {
					checkSourceQualifiedOutcome(value, stoppedClaudeExpectedOutcome);
					return [];
				},
			],
		]),
		quit: {
			key: 'Ctrl+C',
			expected: [
				{
					event: {
						kind: 'key',
						code: { kind: 'shortcut', key: 'c' },
						modifiers: ['control'],
						phase: 'press',
					},
					surface: screen,
					target: null,
					action: 'quit',
				},
			],
		},
	};
}
