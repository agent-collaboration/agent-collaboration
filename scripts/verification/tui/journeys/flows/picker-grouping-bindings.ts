/** R19 observes the actual list, checked menu choices and selection through every regroup. */
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { lookupProcessIdentity } from '../../processes/process-identity.ts';
import type { DispatchFrameObservation } from '../../terminal/input-dispatch-frames.ts';
import type { MarkedInputEvidence } from '../../terminal/marker-input-session.ts';
import { cellText, gridLines } from '../../terminal/terminal-grid.ts';
import type { TerminalGrid } from '../../terminal/terminal-grid.ts';
import { renderOriginal } from '../../terminal/terminal-original.ts';
import { findDialog } from '../scenes/grid-scene-matcher.ts';
import type { AuthoredFlowPlan } from './authored-flow-plan.ts';
import type { FlowInvocation } from './flow-executor.ts';
import type {
	NativeFlowBindings,
	NativeFlowContext,
	NativeFlowInputBinding,
} from './native-flow-runtime.ts';
import {
	pickerBaseBindings,
	pickerExpectedKey,
	pickerOverlay,
	pickerScreen,
} from './picker-flow-inputs.ts';
import {
	checkPickerGroupingObservation,
	pickerGroupedOrder,
	pickerUngroupedOrder,
} from './picker-grouping-oracle.ts';
import type { PickerGroupingFacet } from './picker-grouping-oracle.ts';

const facets: readonly PickerGroupingFacet[] = [
	'none',
	'started-by',
	'day',
	'folder',
	'provider',
	'machine',
];
const facetLabels = ['None', 'Started by', 'Day', 'Folder', 'Provider', 'Machine'];
interface HeadingFact {
	readonly name: string;
	readonly count: number;
}
function listLines(grid: TerminalGrid): string[] {
	const rectangle = findDialog(grid, 'Session');
	if (rectangle === null) throw new Error('grouping list frame absent');
	return grid.cells.slice(rectangle.top + 2, rectangle.bottom).map((row) =>
		row
			.slice(rectangle.left + 1, rectangle.right)
			.map(cellText)
			.join('')
			.replace(/[│█\s]+$/gu, '')
			.trim(),
	);
}
export function groupingSelectedRow(grid: TerminalGrid): string {
	const selected = listLines(grid).filter((line) => line.includes('❯'));
	const labels = pickerUngroupedOrder.filter((label) =>
		selected.some((line) => line.includes(label)),
	);
	if (labels.length !== 1 || labels[0] === undefined)
		throw new Error('grouping selected row is absent, ambiguous or a heading');
	return labels[0];
}
export function groupingVisibleHeadings(grid: TerminalGrid): HeadingFact[] {
	const lines = listLines(grid),
		headings: HeadingFact[] = [];
	for (const [index, line] of lines.entries()) {
		const match = /^(.*?)\s*\(([0-9]+)\)$/u.exec(line);
		if (match === null) continue;
		const prefix = match[1]?.trim() ?? '';
		const name = (prefix.length === 0 ? (lines[index - 1] ?? '') : prefix).trim();
		if (!name || /[❯●]/u.test(name)) throw new Error('group heading acquired a selection marker');
		headings.push({ name, count: Number(match[2]) });
	}
	return headings;
}
function latestFrame(marked: MarkedInputEvidence): DispatchFrameObservation {
	const frame = marked.frames.at(-1);
	if (frame === undefined) throw new Error('grouping input has no marked frame');
	return frame;
}

export function pickerGroupingBindings(
	plan: AuthoredFlowPlan,
	invocation: FlowInvocation,
	evidence: string,
	receipts: string,
	preferences = join(evidence, 'preferences'),
): NativeFlowBindings | null {
	if (
		!['sessions-group-by-f6', 'sessions-group-by-reopened', 'sessions-group-by-fallback'].includes(
			invocation.journey.journeyId,
		)
	)
		return null;
	const base = pickerBaseBindings({
		plan,
		invocation,
		evidence,
		receipts,
		variant: 'r19-grouping',
	});
	let previousSelection: string | null = null,
		nextOverlay = 1,
		inspected = false,
		inspectionOrdinal = 0;
	let bucketFailures: readonly string[] = [],
		selectionFailures: readonly string[] = [];
	let checkedFailures: string[] = [];
	const inputs = new Map<number, NativeFlowInputBinding>();
	for (const [index, step] of invocation.journey.steps.entries())
		if (step.kind === 'press') {
			if (step.key !== 'F6') throw new Error('undeclared grouping key');
			inputs.set(index, {
				kind: 'continuing',
				expected: [pickerExpectedKey('F6', pickerScreen, null, 'cycle-group')],
			});
		}
	const fixtures = new Map<
		string,
		(context: NativeFlowContext) => Promise<MarkedInputEvidence | null>
	>();
	for (const facet of facets)
		fixtures.set('inspect-session-grouping-' + facet, async (context) => {
			const ordinal = ++inspectionOrdinal;
			const entry = context.frame;
			const priorSelection = previousSelection;
			if (!inspected)
				await renderOriginal({
					...base.original,
					output: evidence,
					checkpoint: 'grouping-initial',
					bytes: entry.bytes,
					grid: entry.grid,
					resizes: entry.resizes,
					columns: entry.grid.columns,
					rows: entry.grid.rows,
				});
			const selection = groupingSelectedRow(entry.grid);
			selectionFailures =
				previousSelection !== null && selection !== previousSelection
					? ['grouping changed the previously selected visible identity']
					: [];
			const observedRows: string[] = [],
				headings = new Map<string, number>(),
				frames: number[] = [];
			let marked = await context.input.press('Alt+Shift+Up', [
				pickerExpectedKey('Alt+Shift+Up', pickerScreen, 'sessions-list'),
			]);
			const observe = (): void => {
				const frame = latestFrame(marked);
				frames.push(frame.binding.sequence);
				observedRows.push(groupingSelectedRow(frame.grid));
				for (const heading of groupingVisibleHeadings(frame.grid)) {
					if (headings.has(heading.name) && headings.get(heading.name) !== heading.count)
						throw new Error('group count changed while traversing fixed data');
					headings.set(heading.name, heading.count);
				}
			};
			observe();
			await Array.from({ length: 8 }).reduce(async (previous) => {
				await previous;
				marked = await context.input.press('Down', [
					pickerExpectedKey('Down', pickerScreen, 'sessions-list'),
				]);
				observe();
			}, Promise.resolve());
			bucketFailures = checkPickerGroupingObservation(
				facet,
				observedRows,
				[...headings].map(([name, count]) => ({ name, count })),
			);
			const restoreIndex = pickerGroupedOrder(facet).indexOf(selection);
			if (restoreIndex < 0) throw new Error('selected row is outside declared grouping identities');
			marked = await context.input.press('Alt+Shift+Up', [
				pickerExpectedKey('Alt+Shift+Up', pickerScreen, 'sessions-list'),
			]);
			await Array.from({ length: restoreIndex }).reduce(async (previous) => {
				await previous;
				marked = await context.input.press('Down', [
					pickerExpectedKey('Down', pickerScreen, 'sessions-list'),
				]);
			}, Promise.resolve());
			if (groupingSelectedRow(latestFrame(marked).grid) !== selection)
				selectionFailures = [
					...selectionFailures,
					'row restoration differs from its declared grouping position',
				];
			const menu = pickerOverlay(nextOverlay++);
			marked = await context.input.press('F10', [
				pickerExpectedKey('F10', pickerScreen, null, 'menu'),
			]);
			const choices = new Map<string, boolean>();
			let groupKeyShown = false;
			const collectChoices = async (remaining: number): Promise<void> => {
				for (const line of gridLines(latestFrame(marked).grid)) {
					const offset = line.indexOf('Group by:');
					if (offset >= 0) {
						const label = (line.slice(offset + 'Group by:'.length).split(/[│┃]/u)[0] ?? '').trim();
						choices.set(label, line.slice(Math.max(0, offset - 3), offset).includes('✓'));
					} else if (line.includes('Group by') && line.includes('F6')) {
						if (line.includes('❯')) throw new Error('Group by menu heading became a focus target');
						groupKeyShown = true;
					}
				}
				if (choices.size >= facetLabels.length || remaining === 1) return;
				marked = await context.input.press('Down', [pickerExpectedKey('Down', menu)]);
				await collectChoices(remaining - 1);
			};
			await collectChoices(32);
			checkedFailures = [];
			if (JSON.stringify([...choices.keys()]) !== JSON.stringify(facetLabels) || !groupKeyShown)
				checkedFailures.push('Group by menu lost its closed ordered choices or section F6 key');
			const checked = [...choices].filter(([, value]) => value).map(([label]) => label);
			if (JSON.stringify(checked) !== JSON.stringify([facetLabels[facets.indexOf(facet)]]))
				checkedFailures.push('real menu checked choice differs from expected grouping facet');
			await context.input.press('Escape', [pickerExpectedKey('Escape', menu)]);
			marked = await context.input.press('F12', [pickerExpectedKey('F12', pickerScreen)]);
			if (groupingSelectedRow(latestFrame(marked).grid) !== selection)
				selectionFailures = [
					...selectionFailures,
					'opening and closing grouping menu changed selection',
				];
			previousSelection = selection;
			inspected = true;
			const identity = lookupProcessIdentity(context.driver.processId());
			if (identity === null) throw new Error('owned grouping driver identity unavailable');
			writeFileSync(
				join(evidence, 'grouping-' + ordinal + '-' + facet + '.json'),
				JSON.stringify(
					{
						facet,
						inspectionOrdinal: ordinal,
						entry: entry.binding,
						entrySelectedLabel: selection,
						previousSelectedLabel: priorSelection,
						observedRows,
						headings: [...headings].map(([name, count]) => ({ name, count })),
						traversalSequences: frames,
						choices: [...choices],
						groupKeyShown,
						driverIdentity: identity,
						bucketFailures,
						selectionFailures,
						checkedFailures,
						physicalTerminal: false,
					},
					null,
					2,
				),
			);
			return marked;
		});
	return {
		...base,
		target: { ...base.target, sessionsPreferencesDirectory: preferences },
		inputs,
		fixtures,
		checks: new Map([
			[
				'session-grouping-buckets',
				async () => (inspected ? bucketFailures : ['grouping observation missing']),
			],
			[
				'session-grouping-selection',
				async () => (inspected ? selectionFailures : ['grouping observation missing']),
			],
			[
				'session-grouping-checked',
				async () => (inspected ? checkedFailures : ['grouping observation missing']),
			],
		]),
	};
}
