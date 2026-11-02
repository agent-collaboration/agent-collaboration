/** Independent closed quota menu inventory and acknowledged reveal, from R13 and the declared scope. */
import type { InputConsumptionExpectation } from '../../sessions/input-consumption-schema.ts';
import type { MarkedInputEvidence } from '../../terminal/marker-input-session.ts';
import { cellText } from '../../terminal/terminal-grid.ts';
import type { TerminalGrid } from '../../terminal/terminal-grid.ts';
import { findDialog } from '../scenes/grid-scene-matcher.ts';
import { trackedScrollViewport } from '../scenes/option-navigation-observation.ts';
import type { NativeFlowContext } from './native-flow-runtime.ts';
import { quotaExpectedKey, quotaMarkedFrame } from './quota-flow-inputs.ts';

export const currentQuotaMenu = [
	['details', 'Details', 'F4'],
	['filters', 'Filters…', 'F3'],
	['pool', 'Pool', 'F5'],
	['options', 'Options', 'Ctrl+R'],
	['floor', 'Floor', 'Ctrl+E'],
	['group-none', 'None', ''],
	['group-provider', 'Provider', ''],
	['group-status', 'Status', ''],
	['login', 'Log in again', ''],
	['refresh', 'Refresh', 'R'],
	['help', 'Help', 'F1'],
	['quit', 'Quit', 'Q'],
] as const;
export interface QuotaMenuObservation {
	readonly items: readonly {
		readonly actionId: string;
		readonly title: string;
		readonly key: string;
		readonly checked: boolean;
	}[];
	readonly headings: readonly string[];
	readonly contentEnd: boolean;
}
export function quotaMenuNavigationExpected(
	key: string,
	index: number,
	surface: InputConsumptionExpectation['surface'],
	disabled: readonly string[] = [],
): InputConsumptionExpectation {
	const eligible = currentQuotaMenu.filter(([actionId]) => !disabled.includes(actionId));
	const item = eligible[index % eligible.length];
	if (item === undefined) throw new Error('quota menu focus is outside declared inventory');
	return quotaExpectedKey(key, surface, 'action:quota:' + item[0]);
}
export function readQuotaMenu(grid: TerminalGrid): QuotaMenuObservation {
	const rectangle = findDialog(grid, '');
	if (rectangle === null) throw new Error('quota menu border absent');
	const items: QuotaMenuObservation['items'][number][] = [],
		headings: string[] = [];
	for (const row of grid.cells.slice(rectangle.top + 1, rectangle.bottom)) {
		const line = row
			.slice(rectangle.left + 1, rectangle.right)
			.map(cellText)
			.join('')
			.replace(/[│█\s]+$/gu, '')
			.trim();
		if (line === '' || /^[─━┄┈]+$/u.test(line)) continue;
		const text = line.replace(/^[✓❯]\s*/u, '');
		if (text.startsWith('Group by') && text.endsWith('F6')) {
			headings.push('Group by');
			continue;
		}
		const item = currentQuotaMenu.find(
			([, title]) => text === title || text.startsWith(title + ' '),
		);
		if (item === undefined) throw new Error('unexpected quota menu row: ' + text);
		items.push({
			actionId: item[0],
			title: item[1],
			key: text.slice(item[1].length).trim(),
			checked: line.startsWith('✓'),
		});
	}
	const viewport = trackedScrollViewport(grid, rectangle);
	return {
		items,
		headings,
		contentEnd:
			items.some((item) => item.actionId === 'quit') && (viewport === null || viewport.atEnd),
	};
}
export async function collectQuotaMenu(options: {
	readonly context: NativeFlowContext;
	readonly surface: InputConsumptionExpectation['surface'];
	readonly disabled?: readonly string[];
}): Promise<{ readonly marked: MarkedInputEvidence; readonly observation: QuotaMenuObservation }> {
	const { context, surface } = options;
	const disabled = options.disabled ?? [];
	let marked = await context.input.press('F12', [quotaExpectedKey('F12', surface)]);
	const items = new Map<string, QuotaMenuObservation['items'][number]>(),
		headings = new Set<string>();
	let contentEnd = false;
	const inspect = (): void => {
		const view = readQuotaMenu(quotaMarkedFrame(marked).grid);
		for (const item of view.items) {
			const prior = items.get(item.actionId);
			if (prior !== undefined && JSON.stringify(prior) !== JSON.stringify(item))
				throw new Error('quota menu changed during a fixed inventory census');
			items.set(item.actionId, item);
		}
		for (const heading of view.headings) headings.add(heading);
		contentEnd ||= view.contentEnd;
	};
	inspect();
	const eligible = currentQuotaMenu.filter(([actionId]) => !disabled.includes(actionId));
	await Array.from({ length: eligible.length * 2 }, (_, index) => index).reduce(
		async (previous, index) => {
			await previous;
			marked = await context.input.press('Down', [
				quotaMenuNavigationExpected('Down', index, surface, disabled),
			]);
			inspect();
		},
		Promise.resolve(),
	);
	const observed = [...items.values()];
	for (const [actionId, title, key] of currentQuotaMenu) {
		const found = items.get(actionId);
		if (
			found === undefined ||
			found.title !== title ||
			(!disabled.includes(actionId) && found.key !== key)
		)
			throw new Error('quota closed menu inventory/key differs: ' + actionId);
	}
	if (observed.length !== currentQuotaMenu.length || !headings.has('Group by') || !contentEnd)
		throw new Error('quota menu census missed its heading, inventory or painted content end');
	return { marked, observation: { items: observed, headings: [...headings], contentEnd } };
}
