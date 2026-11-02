/** Full closed-set menu observations, including nonfocusable headings and checked settings. */
import type { InputConsumptionExpectation } from '../../sessions/input-consumption-schema.ts';
import type { MarkedInputEvidence } from '../../terminal/marker-input-session.ts';
import { cellText, terminalTheme } from '../../terminal/terminal-grid.ts';
import type { TerminalGrid } from '../../terminal/terminal-grid.ts';
import { findDialog } from '../scenes/grid-scene-matcher.ts';
import { trackedScrollViewport } from '../scenes/option-navigation-observation.ts';
import type { NativeFlowContext } from './native-flow-runtime.ts';
import { pickerExpectedKey } from './picker-flow-inputs.ts';
import { pickerMarkedFrame } from './picker-help-observation.ts';

const pickerMenuActions = [
	['new', 'New session…', 'Ctrl+N'],
	['focus-filter', 'Filter', '/'],
	['scope-folder', 'This folder', ''],
	['scope-repo', 'Repo', ''],
	['scope-all', 'All', ''],
	['runtime-all', 'Runtime: All', ''],
	['runtime-active', 'Runtime: Active', ''],
	['runtime-idle', 'Runtime: Idle', ''],
	['runtime-blocked', 'Runtime: Blocked', ''],
	['sort-updated', 'Sort: Updated', ''],
	['sort-created', 'Sort: Created', ''],
	['group-none', 'Group by: None', ''],
	['group-started-by', 'Group by: Started by', ''],
	['group-day', 'Group by: Day', ''],
	['group-folder', 'Group by: Folder', ''],
	['group-provider', 'Group by: Provider', ''],
	['group-machine', 'Group by: Machine', ''],
	['new-background', 'New background session…', ''],
	['details', 'Details', 'Enter'],
	['open', 'Open', 'Ctrl+Enter'],
	['fork', 'Fork', 'Ctrl+Shift+Enter'],
	['refresh', 'Refresh', 'Ctrl+R'],
	['filters', 'Filters…', 'Ctrl+F'],
	['machine', 'Machine…', 'Ctrl+G'],
	['help', 'Help', 'F1'],
	['quit', 'Quit', 'Ctrl+C'],
] as const;
export const currentPickerMenu: readonly (readonly [string, string])[] = pickerMenuActions.map(
	([, title, key]): readonly [string, string] => [title, key],
);

export function pickerMenuNavigationExpectation(options: {
	readonly key: string;
	readonly focusedIndex: number;
	readonly surface: InputConsumptionExpectation['surface'];
}): InputConsumptionExpectation {
	if (
		!Number.isSafeInteger(options.focusedIndex) ||
		options.focusedIndex < 0 ||
		!['Down', 'Up', 'Left', 'Right', 'Tab', 'Shift+Tab'].includes(options.key)
	)
		throw new Error('menu navigation needs a declared key and previous focus index');
	// The fixed fixture has all 26 actions eligible. A fresh menu starts at New;
	// FocusModel wraps through this closed order. Used reports the pre-key target.
	const action = pickerMenuActions[options.focusedIndex % pickerMenuActions.length];
	if (action === undefined) throw new Error('menu action outside the closed inventory');
	return pickerExpectedKey(options.key, options.surface, 'action:' + action[0]);
}
const sections = [
	['Scope', 'Ctrl+S'],
	['Runtime', 'Ctrl+T'],
	['Sort', 'Ctrl+O'],
	['Group by', 'F6'],
] as const;
export interface PickerMenuFact {
	readonly title: string;
	readonly key: string;
	readonly checked: boolean;
}
export interface PickerMenuObservation {
	readonly items: readonly PickerMenuFact[];
	readonly headings: readonly string[];
	readonly finalChoiceAtContentEnd: boolean;
	readonly failures: readonly string[];
}

export function readPickerMenuViewport(grid: TerminalGrid): {
	items: PickerMenuFact[];
	headings: string[];
	quitAtEnd: boolean;
} {
	const rectangle = findDialog(grid, '');
	if (rectangle === null) throw new Error('painted menu border is absent');
	const items: PickerMenuFact[] = [],
		headings: string[] = [];
	let quitFocused = false;
	for (const row of grid.cells.slice(rectangle.top + 1, rectangle.bottom)) {
		const cells = row.slice(rectangle.left + 1, rectangle.right);
		const line = cells
			.map(cellText)
			.join('')
			.replace(/[│█\s]+$/gu, '')
			.trim();
		if (line === '' || /^[─━┄┈]+$/u.test(line)) continue;
		const text = line.replace(/^[✓❯]\s*/u, '');
		const item = currentPickerMenu.find(
			([title]) => text === title || text.startsWith(title + ' '),
		);
		if (item !== undefined) {
			const key = text.slice(item[0].length).trim();
			items.push({ title: item[0], key, checked: line.startsWith('✓') });
			if (item[0] === 'Quit' && cells.some((cell) => cell.fgRgb === terminalTheme.palette[11]))
				quitFocused = true;
			continue;
		}
		const section = sections.find(([title, key]) => text.startsWith(title) && text.endsWith(key));
		if (section !== undefined) {
			if (line.startsWith('❯')) throw new Error('menu section heading became focusable');
			headings.push(section[0]);
		} else throw new Error('unexpected painted picker menu row: ' + text);
	}
	const viewport = trackedScrollViewport(grid, rectangle);
	return { items, headings, quitAtEnd: quitFocused && (viewport === null || viewport.atEnd) };
}

export async function collectPickerMenu(options: {
	readonly context: NativeFlowContext;
	readonly surface: InputConsumptionExpectation['surface'];
}): Promise<{ readonly marked: MarkedInputEvidence; readonly observation: PickerMenuObservation }> {
	const { context, surface } = options;
	let marked = await context.input.press('F12', [pickerExpectedKey('F12', surface)]);
	const items = new Map<string, PickerMenuFact>(),
		headings = new Set<string>();
	let finalChoiceAtContentEnd = false;
	const inspect = (): void => {
		const view = readPickerMenuViewport(pickerMarkedFrame(marked).grid);
		for (const item of view.items) {
			const prior = items.get(item.title);
			if (prior !== undefined && JSON.stringify(prior) !== JSON.stringify(item))
				throw new Error('menu choice changed while observing a fixed fixture');
			items.set(item.title, item);
		}
		for (const heading of view.headings) headings.add(heading);
		finalChoiceAtContentEnd ||= view.quitAtEnd;
	};
	inspect();
	await Array.from({ length: 64 }, (_, focusedIndex) => focusedIndex).reduce(
		async (previous, focusedIndex) => {
			await previous;
			marked = await context.input.press('Down', [
				pickerMenuNavigationExpectation({ key: 'Down', focusedIndex, surface }),
			]);
			inspect();
		},
		Promise.resolve(),
	);
	const actual = [...items.values()];
	const failures: string[] = [];
	if (
		JSON.stringify(actual.map((item) => [item.title, item.key])) !==
		JSON.stringify(currentPickerMenu)
	)
		failures.push('menu differs from the closed ordered action/key inventory');
	if (JSON.stringify([...headings]) !== JSON.stringify(sections.map(([title]) => title)))
		failures.push('menu section keys are missing or out of order');
	if (!finalChoiceAtContentEnd) failures.push('Quit did not reach the actual menu content end');
	return {
		marked,
		observation: { items: actual, headings: [...headings], finalChoiceAtContentEnd, failures },
	};
}
