/** Source-qualified picker/Help paging, including real same-child resize and original captures. */
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
import type { TerminalGrid } from '../../terminal/terminal-grid.ts';
import { gridLines } from '../../terminal/terminal-grid.ts';
import { renderOriginal } from '../../terminal/terminal-original.ts';
import { journeyGridSizes } from '../journey-definition.ts';
import { findDialog } from '../scenes/grid-scene-matcher.ts';
import {
	helpViewport,
	pickerPageRows,
	requireHelpNoop,
	requireHelpRows,
	selectedFixtureRow,
} from '../scenes/option-navigation-observation.ts';
import type { AuthoredFlowPlan } from './authored-flow-plan.ts';
import type { FlowCheck } from './flow-definition.ts';
import type { FlowInvocation } from './flow-executor.ts';
import type {
	NativeFlowBindings,
	NativeFlowContext,
	NativeFlowInputBinding,
} from './native-flow-runtime.ts';
import {
	pickerBaseBindings,
	pickerExpectedKey,
	pickerFilterTextExpectations,
	pickerOverlay,
	pickerScreen,
	resizePickerWhenChanged,
} from './picker-flow-inputs.ts';
import { qualifyPickerHelp } from './picker-help-observation.ts';
import type { QualifiedPickerHelp } from './picker-help-observation.ts';
import {
	pickerOptionPageRows,
	pickerOptionRows,
	requirePickerOptionIndex,
} from './picker-option-oracle.ts';

const aliases: readonly string[] = ['Alt+Shift+Up', 'Alt+Down', 'Alt+Up', 'Alt+Shift+Down'];
const helpSurface = pickerOverlay(1);
const helpBody = '11:action:help:help-body';

function markedFrame(marked: MarkedInputEvidence): NativeFlowContext['frame'] {
	const frame = marked.frames.at(-1);
	if (frame === undefined) throw new Error('navigation input has no marked frame');
	return frame;
}
function navigationExpected(key: string, help: boolean): InputConsumptionExpectation {
	return pickerExpectedKey(
		key,
		help ? helpSurface : pickerScreen,
		help ? helpBody : 'sessions-list',
	);
}

function requireListPage(grid: TerminalGrid): number {
	const dimensions = `${grid.columns}x${grid.rows}`;
	const declared = journeyGridSizes.find((size) => size === dimensions);
	if (declared === undefined) throw new Error('undeclared navigation grid');
	const page = pickerOptionPageRows[declared];
	if (pickerPageRows(grid) !== page)
		throw new Error('physical list page differs from its literal per-grid budget');
	return page;
}

export function pickerOptionBindings(
	plan: AuthoredFlowPlan,
	invocation: FlowInvocation,
	evidence: string,
	receipts: string,
): NativeFlowBindings | null {
	const name = invocation.journey.journeyId;
	if (name !== 'picker-list-option-navigation' && name !== 'sessions-help-option-navigation')
		return null;
	const help = name === 'sessions-help-option-navigation';
	const base = pickerBaseBindings({ plan, invocation, evidence, receipts, variant: 'large-list' });
	let child: ProcessIdentity | null = null;
	let qualified: QualifiedPickerHelp | null = null;
	let checkOrdinal = 0;
	let helpQualificationOrdinal = 0;
	const checkName: FlowCheck = help ? 'help-option-page-and-ends' : 'picker-option-page-and-ends';
	const checkpoints = invocation.journey.steps.flatMap((step) =>
		step.kind === 'capture' && step.checks.includes(checkName) ? [step.checkpoint] : [],
	);
	const inputs = new Map<number, NativeFlowInputBinding>();
	for (const [index, step] of invocation.journey.steps.entries()) {
		if (step.kind === 'focus')
			inputs.set(index, {
				kind: 'focus',
				stopWhenFocused: true,
				route: Array.from({ length: 12 }, () => ({
					key: 'Tab',
					expected: [pickerExpectedKey('Tab', pickerScreen)],
				})),
				isFocused: (grid) => {
					try {
						selectedFixtureRow(grid, pickerOptionRows);
						return true;
					} catch {
						return false;
					}
				},
			});
		if (step.kind === 'press')
			inputs.set(index, {
				kind: 'continuing',
				expected: [
					step.key === 'F1'
						? pickerExpectedKey('F1', pickerScreen, null, 'help')
						: navigationExpected(step.key, help),
				],
			});
	}
	const retainChild = (): void => {
		if (child === null || !sameIdentity(lookupProcessIdentity(child.pid), child))
			throw new Error('same-PTY navigation lost its exact product PID/UID/birth/executable');
	};
	const qualifyHelp = async (context: NativeFlowContext): Promise<MarkedInputEvidence> => {
		const observation = await qualifyPickerHelp({ context, surface: helpSurface });
		qualified = observation.qualified;
		const first = observation.entry;
		const dimensions = first.grid.columns + 'x' + first.grid.rows;
		writeFileSync(
			join(
				evidence,
				'help-physical-oracle-' + ++helpQualificationOrdinal + '-' + dimensions + '.json',
			),
			JSON.stringify(
				{
					basis:
						'physical one-row transitions checked against independent full census, not application scroll counters',
					qualification: qualified,
					child,
					entry: first.binding,
				},
				null,
				2,
			),
		);
		return observation.marked;
	};
	const requireHelpDestination = (grid: TerminalGrid, checkpoint: string): void => {
		if (qualified === null) throw new Error('Help physical rows were not qualified');
		if (qualified.kind === 'fit') {
			requireHelpNoop(qualified.grid, grid);
			return;
		}
		const maximum = qualified.full.length - qualified.height;
		const offset =
			checkpoint === 'alias-last'
				? maximum
				: checkpoint === 'alias-page-down'
					? Math.min(maximum, qualified.height)
					: 0;
		requireHelpRows(
			helpViewport(grid).rows,
			qualified.full.slice(offset, offset + qualified.height),
		);
	};
	return {
		...base,
		inputs,
		targets: new Map([
			[
				help ? 'main list before Help' : 'main list',
				(grid) => {
					const bounds = findDialog(grid, 'Session');
					if (bounds === null) throw new Error('main list target is not painted');
					return { label: 'Session', bounds };
				},
			],
		]),
		fixtures: new Map<string, (context: NativeFlowContext) => Promise<MarkedInputEvidence | null>>([
			[
				'seed-option-navigation-' + (help ? 'sessions-help' : 'picker-list'),
				async (context) => {
					child = requireOwnedProductChild(context.driver.processId(), plan.product.path);
					requirePickerOptionIndex(selectedFixtureRow(context.frame.grid, pickerOptionRows), 0);
					writeFileSync(
						join(evidence, 'option-child-binding.json'),
						JSON.stringify(
							{ child, driverPid: context.driver.processId(), product: plan.product },
							null,
							2,
						),
					);
					await context.input.press('/', [
						{
							event: { kind: 'key', code: { kind: 'text' }, modifiers: [], phase: 'press' },
							surface: pickerScreen,
							target: null,
							action: 'focus-filter',
						},
					]);
					return context.input.text(
						'Large',
						pickerFilterTextExpectations('Large', context.driver.inputModes().kittyFlags),
					);
				},
			],
			['qualify-native-help-physical-rows', qualifyHelp],
			[
				'try-consumed-original-navigation-keys',
				async (context) => {
					const start = context.input.input.history().length;
					await ['PageUp', 'PageDown', 'Home', 'End'].reduce(async (previous, key) => {
						await previous;
						if ((await context.input.input.key(key)).delivery.kind !== 'consumed')
							throw new Error('example profile did not consume original navigation key');
					}, Promise.resolve());
					writeFileSync(
						join(evidence, 'consumed-original-keys.json'),
						JSON.stringify(context.input.input.history().slice(start), null, 2),
					);
					return null;
				},
			],
			[
				'resize-same-child-to-each-declared-grid',
				async (context) => {
					let marked: MarkedInputEvidence | null = null;
					await journeyGridSizes.reduce(async (previous, dimensions) => {
						await previous;
						retainChild();
						const [columns, rows] = dimensions.split('x').map(Number);
						if (columns === undefined || rows === undefined)
							throw new Error('navigation dimensions absent');
						marked = await resizePickerWhenChanged({
							context: { ...context, frame: marked === null ? context.frame : markedFrame(marked) },
							columns,
							rows,
							surface: help ? helpSurface : pickerScreen,
						});
						if (help) marked = await qualifyHelp({ ...context, frame: markedFrame(marked) });
						const page = help ? 0 : requireListPage(markedFrame(marked).grid);
						await aliases.reduce(async (prior, key) => {
							await prior;
							marked = await context.input.press(key, [navigationExpected(key, help)]);
							const checkpoint =
								key === 'Alt+Down'
									? 'alias-page-down'
									: key === 'Alt+Shift+Down'
										? 'alias-last'
										: 'alias-first';
							if (help) requireHelpDestination(markedFrame(marked).grid, checkpoint);
							else
								requirePickerOptionIndex(
									selectedFixtureRow(markedFrame(marked).grid, pickerOptionRows),
									key === 'Alt+Down' ? page : key === 'Alt+Shift+Down' ? 59 : 0,
								);
						}, Promise.resolve());
						if (marked === null) throw new Error('resize navigation has no final frame');
						const frame = markedFrame(marked);
						await renderOriginal({
							...base.original,
							output: evidence,
							checkpoint: 'option-resize-' + dimensions,
							bytes: frame.bytes,
							grid: frame.grid,
							resizes: frame.resizes,
							columns,
							rows,
						});
						retainChild();
					}, Promise.resolve());
					return marked;
				},
			],
		]),
		checks: new Map([
			[
				checkName,
				async (context) => {
					retainChild();
					const checkpoint = checkpoints[checkOrdinal++];
					if (checkpoint === undefined) throw new Error('navigation checkpoint not authored');
					if (checkpoint === 'alias-help-return') {
						if (findDialog(context.frame.grid, 'Help') !== null) return ['Help did not close'];
						requirePickerOptionIndex(selectedFixtureRow(context.frame.grid, pickerOptionRows), 0);
						const toolbar = gridLines(context.frame.grid).find(
							(line) => line.includes('≡') && line.includes('F10'),
						);
						if (toolbar?.includes('Large') !== true) return ['Help lost the covered query'];
					} else if (help) requireHelpDestination(context.frame.grid, checkpoint);
					else {
						const page = requireListPage(context.frame.grid);
						requirePickerOptionIndex(
							selectedFixtureRow(context.frame.grid, pickerOptionRows),
							checkpoint === 'alias-last' ? 59 : checkpoint === 'alias-page-down' ? page : 0,
						);
					}
					return [];
				},
			],
			[
				'option-alias-delivery',
				async (context) => {
					const record = context.input.input.history().at(-1);
					if (
						record?.input.kind !== 'key' ||
						!aliases.includes(record.input.key) ||
						record.delivery.kind !== 'sent'
					)
						return ['Option alias was not delivered'];
					const modifier = record.input.key.includes('Shift') ? 4 : 3;
					const final = record.input.key.endsWith('Up') ? 'A' : 'B';
					const expected = `\x1b[1;${modifier}${invocation.keys === 'kitty' ? ':1' : ''}${final}`;
					return record.delivery.hex === Buffer.from(expected).toString('hex')
						? []
						: ['Option alias bytes lost modifiers or press-event suffix'];
				},
			],
		]),
	};
}
