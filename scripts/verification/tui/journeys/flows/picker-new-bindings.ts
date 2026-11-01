/** New's real dropdown and returned/prepared outcome bindings; no provider launch. */
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

import type { InputConsumptionExpectation } from '../../sessions/input-consumption-schema.ts';
import { checkPreparationReadback } from '../../sessions/preparation-readback.ts';
import {
	checkSourceQualifiedOutcome,
	parsePickerOutcomeReadback,
} from '../../sessions/source-qualified-outcome.ts';
import { cellText, terminalTheme } from '../../terminal/terminal-grid.ts';
import type { TerminalGrid } from '../../terminal/terminal-grid.ts';
import { findDialog, gridFieldPair } from '../scenes/grid-scene-matcher.ts';
import type { AuthoredFlowPlan } from './authored-flow-plan.ts';
import type { FlowCheck } from './flow-definition.ts';
import type { FlowInvocation } from './flow-executor.ts';
import type {
	NativeFlowBindings,
	NativeFlowInputBinding,
	NativeFlowContext,
} from './native-flow-runtime.ts';
import {
	pickerBaseBindings,
	pickerDialogTarget,
	pickerExpectedKey,
	pickerFocused,
	pickerOverlay,
	pickerPrimaryTarget,
	pickerScreen,
} from './picker-flow-inputs.ts';

type NewSource = 'default' | 'local' | 'configured';
function newSource(journey: string): NewSource | null {
	if (!journey.startsWith('new-')) return null;
	for (const source of ['default', 'local', 'configured'] as const)
		if (journey.endsWith('-' + source)) return source;
	return null;
}
export function checkNewMachineDropdown(grid: TerminalGrid, source: NewSource): readonly string[] {
	const rectangle = findDialog(grid, 'Filter:');
	if (rectangle === null) return ['Machine dropdown is not painted'];
	const selected =
		source === 'local'
			? 'Local Codex'
			: source === 'configured'
				? 'Fixture remote'
				: 'This machine';
	const highlighted =
		source === 'configured'
			? 'This machine'
			: source === 'default'
				? 'Fixture remote'
				: 'Local Codex';
	const rows = grid.cells.slice(rectangle.top + 1, rectangle.bottom).map((cells) => ({
		cells: cells.slice(rectangle.left + 1, rectangle.right),
		text: cells
			.slice(rectangle.left + 1, rectangle.right)
			.map(cellText)
			.join(''),
	}));
	const choices = rows.filter((row) => /[●○]/u.test(row.text));
	const failures: string[] = [];
	if (source === 'local' && choices.length !== 1)
		failures.push('LocalCodex must expose exactly one Machine option');
	if (!choices.some((row) => row.text.includes('●') && row.text.includes(selected)))
		failures.push('committed Machine mark changed before Start');
	if (
		!choices.some(
			(row) =>
				row.text.includes(highlighted) &&
				row.cells.some((cell) => cell.fgRgb === terminalTheme.palette[11]),
		)
	)
		failures.push('declared highlighted Machine is not visibly focused');
	if (source !== 'local' && selected === highlighted)
		failures.push('fixture did not reach a distinct uncommitted Machine');
	return failures;
}
export function pickerNewBindings(
	plan: AuthoredFlowPlan,
	invocation: FlowInvocation,
	evidence: string,
	receipts: string,
): NativeFlowBindings | null {
	const source = newSource(invocation.journey.journeyId);
	if (source === null) return null;
	const base = pickerBaseBindings({
		plan,
		invocation,
		evidence,
		receipts,
		variant: source === 'local' ? 'local-codex-new' : 'new-machine-uncommitted',
	});
	const dialog = pickerOverlay(1),
		dropdown = pickerOverlay(source === 'configured' ? 3 : 2);
	const machine =
		source === 'local'
			? 'Local Codex'
			: source === 'configured'
				? 'Fixture remote'
				: 'This machine';
	const expectedRequest = {
		client: 'codex',
		machine:
			source === 'local'
				? 'localCodex'
				: source === 'configured'
					? 'configured:Fixture remote'
					: 'defaultHosted',
		directory: source === 'configured' ? '/fixture/remote' : '/repo/project-a',
		runMode: 'foreground',
	} as const;
	const inputs = new Map<number, NativeFlowInputBinding>();
	for (const [index, step] of invocation.journey.steps.entries()) {
		if (step.kind === 'press') {
			const previous = invocation.journey.steps[index - 1];
			let expected: InputConsumptionExpectation;
			let ends = false;
			if (step.key === 'Ctrl+N') expected = pickerExpectedKey(step.key, pickerScreen, null, 'new');
			else if (step.key === 'Ctrl+Enter') {
				expected = pickerExpectedKey(step.key, dialog, null, 'new-start');
				ends = true;
			} else if (step.key === 'Escape') expected = pickerExpectedKey(step.key, dropdown);
			else if (step.key === 'Down' || step.key === 'Up')
				expected = pickerExpectedKey(step.key, dropdown, '11:new-machine:filter');
			else if (step.key === 'Enter' && previous?.kind === 'focus' && previous.target === 'Machine')
				expected = pickerExpectedKey(step.key, dialog, 'new-machine', 'new-machine');
			else if (step.key === 'Enter') {
				expected = pickerExpectedKey(step.key, dialog, 'action:new-start', 'new-start');
				ends = true;
			} else throw new Error('New binding has no declared key route: ' + step.key);
			inputs.set(index, { kind: ends ? 'exit' : 'continuing', expected: [expected] });
		} else if (step.kind === 'focus') {
			const isMachine = step.target === 'Machine';
			const key = isMachine && step.via === 'arrows' ? 'Down' : 'Tab';
			inputs.set(index, {
				kind: 'focus',
				route: Array.from({ length: isMachine ? 1 : 3 }, () => ({
					key,
					expected: [pickerExpectedKey(key, dialog)],
				})),
				stopWhenFocused: true,
				isFocused: (grid) => pickerFocused(grid, 'New session', isMachine ? machine : 'Start'),
			});
		} else if (step.kind === 'click') {
			inputs.set(index, {
				kind: 'click',
				ending: 'process-exit',
				expected: (location) => [
					{
						event: {
							kind: 'mouse',
							mouse: { kind: 'down', button: 'left' },
							column: location.column,
							row: location.row,
							modifiers: [],
						},
						surface: dialog,
						target: 'action:new-start',
						action: 'new-start',
					},
				],
			});
		}
	}
	const checkOutcome = async (_context: unknown, observed: unknown): Promise<readonly string[]> => {
		if (source === 'configured') checkPreparationReadback(observed, expectedRequest);
		else checkSourceQualifiedOutcome(observed, { kind: 'new', request: expectedRequest });
		return [];
	};
	const checks = new Map<
		FlowCheck,
		(context: NativeFlowContext, observed: unknown) => Promise<readonly string[]>
	>([
		['compound-start-once', checkOutcome],
		['legacy-start-fallback', checkOutcome],
		['returned-new-once', checkOutcome],
		['configured-start-once', checkOutcome],
		[
			'dropdown-highlight-uncommitted',
			async (context) => checkNewMachineDropdown(context.frame.grid, source),
		],
		[
			'single-local-machine',
			async (context) => checkNewMachineDropdown(context.frame.grid, 'local'),
		],
	]);
	return {
		...base,
		inputs,
		targets: new Map([
			['Machine', (grid) => pickerDialogTarget(grid, 'New session', machine)],
			['Start', (grid) => pickerPrimaryTarget(grid, 'New session', 'Start')],
		]),
		fixtures: new Map([
			[
				'select-New-source:' + expectedRequest.machine,
				async (context) => {
					if (
						!gridFieldPair(
							context.frame.grid,
							'New session',
							'Machine',
							source === 'local' ? 'Local Codex' : 'This machine',
						)
					)
						throw new Error('New initial Machine differs from its independent fixture');
					if (source !== 'configured') return null;
					await context.input.press('Tab', [pickerExpectedKey('Tab', dialog)]);
					await context.input.press('Enter', [
						pickerExpectedKey('Enter', dialog, 'new-machine', 'new-machine'),
					]);
					await context.input.press('Down', [
						pickerExpectedKey('Down', pickerOverlay(2), '11:new-machine:filter'),
					]);
					const picked = await context.input.press('Enter', [
						pickerExpectedKey('Enter', pickerOverlay(2), '11:new-machine:filter'),
					]);
					const frame = picked.frames.at(-1);
					if (
						frame === undefined ||
						!gridFieldPair(frame.grid, 'New session', 'Machine', 'Fixture remote')
					)
						throw new Error('configured Machine setup did not commit the remote source');
					return picked;
				},
			],
		]),
		readbacks: new Map([
			[
				'source-qualified-outcome',
				async () => parsePickerOutcomeReadback(readFileSync(join(receipts, 'picker-outcome.json'))),
			],
			[
				'preparation',
				async () => {
					const value: unknown = JSON.parse(
						readFileSync(join(receipts, 'source-preparation.json'), 'utf8'),
					);
					return value;
				},
			],
		]),
		checks,
		checkpointBarriers: new Map(),
	};
}
