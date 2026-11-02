/** Index actual physical list rows with one-row wheel input; aliases never compute their own oracle. */
import type { InputConsumptionExpectation } from '../../sessions/input-consumption-schema.ts';
import type { MarkedInputEvidence } from '../../terminal/marker-input-session.ts';
import type { TerminalGrid } from '../../terminal/terminal-grid.ts';
import { gridLines } from '../../terminal/terminal-grid.ts';
import { findDialog } from '../scenes/grid-scene-matcher.ts';
import {
	appendOneHelpRow,
	trackedScrollViewport,
} from '../scenes/option-navigation-observation.ts';
import type { HelpViewport } from '../scenes/option-navigation-observation.ts';
import type { NativeFlowContext } from './native-flow-runtime.ts';
import { quotaExpectedKey, quotaMarkedFrame, quotaScreen } from './quota-flow-inputs.ts';
export function quotaContentViewport(grid: TerminalGrid): HelpViewport {
	const rectangle = findDialog(grid, '');
	if (rectangle === null) throw new Error('quota content frame absent');
	const viewport = trackedScrollViewport(grid, rectangle);
	if (viewport === null) throw new Error('quota physical scroll track absent');
	return viewport;
}
export function quotaWheelExpected(
	direction: 'up' | 'down',
	column: number,
	row: number,
): InputConsumptionExpectation {
	return {
		event: {
			kind: 'mouse',
			mouse: { kind: direction === 'up' ? 'scrollUp' : 'scrollDown' },
			column,
			row,
			modifiers: [],
		},
		surface: quotaScreen,
		target: '21:action:quota:accounts:scroll',
		action: null,
	};
}
export async function collectQuotaContent(context: NativeFlowContext): Promise<{
	readonly marked: MarkedInputEvidence;
	readonly full: readonly string[];
	readonly height: number;
}> {
	let marked = await context.input.press('F12', [quotaExpectedKey('F12', quotaScreen)]);
	const grid = quotaMarkedFrame(marked).grid,
		rectangle = findDialog(grid, '');
	if (rectangle === null) throw new Error('quota content frame absent');
	const first = trackedScrollViewport(grid, rectangle);
	if (first === null) {
		const full = gridLines(grid).slice(rectangle.top + 1, rectangle.bottom);
		return { marked, full, height: full.length };
	}
	let viewport = first;
	const initial = [...viewport.rows],
		full = [...initial],
		height = initial.length;
	let moved = 0;
	const wheel = async (direction: 'up' | 'down'): Promise<void> => {
		const currentRectangle = quotaContentViewport(quotaMarkedFrame(marked).grid).rectangle,
			column = currentRectangle.left + 2,
			row = currentRectangle.top + 2;
		marked = await context.input.pointer({ kind: 'wheel', direction, column, row }, [
			quotaWheelExpected(direction, column, row),
		]);
	};
	const advance = async (): Promise<void> => {
		if (viewport.atEnd) return;
		if (moved >= 4096) throw new Error('quota physical content exceeds the bounded census');
		await wheel('down');
		const next = quotaContentViewport(quotaMarkedFrame(marked).grid);
		full.push(appendOneHelpRow(viewport.rows, next.rows));
		viewport = next;
		moved += 1;
		await advance();
	};
	await advance();
	await Array.from({ length: moved }).reduce(async (previous) => {
		await previous;
		await wheel('up');
	}, Promise.resolve());
	const restored = quotaContentViewport(quotaMarkedFrame(marked).grid);
	if (JSON.stringify(restored.rows) !== JSON.stringify(initial))
		throw new Error('quota physical census did not restore its entry rows');
	return { marked, full, height };
}
