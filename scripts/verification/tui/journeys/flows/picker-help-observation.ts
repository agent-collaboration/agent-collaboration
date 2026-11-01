/** One physical Help census collector shared by paging and menu-parity observations. */
import type { InputConsumptionExpectation } from '../../sessions/input-consumption-schema.ts';
import type { MarkedInputEvidence } from '../../terminal/marker-input-session.ts';
import type { TerminalGrid } from '../../terminal/terminal-grid.ts';
import {
	appendOneHelpRow,
	helpLayout,
	helpViewport,
	requireFullHelpCensus,
	requireHelpRows,
} from '../scenes/option-navigation-observation.ts';
import type { NativeFlowContext } from './native-flow-runtime.ts';
import { pickerExpectedKey } from './picker-flow-inputs.ts';
import { currentPickerHelpCensus } from './picker-option-oracle.ts';

export type QualifiedPickerHelp =
	| { readonly kind: 'fit'; readonly grid: TerminalGrid }
	| { readonly kind: 'overflow'; readonly full: readonly string[]; readonly height: number };

export function pickerMarkedFrame(marked: MarkedInputEvidence): NativeFlowContext['frame'] {
	const frame = marked.frames.at(-1);
	if (frame === undefined) throw new Error('picker observation input has no marked frame');
	return frame;
}

export async function qualifyPickerHelp(options: {
	readonly context: NativeFlowContext;
	readonly surface: InputConsumptionExpectation['surface'];
}): Promise<{
	readonly marked: MarkedInputEvidence;
	readonly qualified: QualifiedPickerHelp;
	readonly entry: NativeFlowContext['frame'];
}> {
	const { context, surface } = options;
	const expected = (key: string): InputConsumptionExpectation =>
		pickerExpectedKey(key, surface, '11:action:help:help-body');
	let marked = await context.input.press('Alt+Shift+Up', [expected('Alt+Shift+Up')]);
	const entry = pickerMarkedFrame(marked);
	const layout = helpLayout(entry.grid, currentPickerHelpCensus);
	if (layout.kind === 'incomplete') throw new Error(layout.reason);
	if (layout.kind === 'fit') return { marked, qualified: { kind: 'fit', grid: entry.grid }, entry };
	const initial = layout.viewport;
	if (!initial.rows.join(' ').includes('Context')) throw new Error('Help first content is absent');
	const full = [...initial.rows];
	const collect = async (remaining: number): Promise<void> => {
		const before = helpViewport(pickerMarkedFrame(marked).grid);
		if (before.atEnd) return;
		if (remaining === 0) throw new Error('bounded Help physical-row census exhausted');
		marked = await context.input.press('Down', [expected('Down')]);
		full.push(appendOneHelpRow(before.rows, helpViewport(pickerMarkedFrame(marked).grid).rows));
		await collect(remaining - 1);
	};
	await collect(2048);
	requireFullHelpCensus(full, currentPickerHelpCensus);
	marked = await context.input.press('Alt+Shift+Up', [expected('Alt+Shift+Up')]);
	requireHelpRows(
		helpViewport(pickerMarkedFrame(marked).grid).rows,
		full.slice(0, initial.rows.length),
	);
	return { marked, qualified: { kind: 'overflow', full, height: initial.rows.length }, entry };
}
