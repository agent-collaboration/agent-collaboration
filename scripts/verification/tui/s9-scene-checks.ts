import { findDialog } from './journeys/scenes/grid-scene-matcher.ts';
/** Grid and projection assertions for future entries; unit fixtures are not product proof. */
import type { ActionProjection } from './s9-picker-action-inventory.ts';
import {
	pickerBaseActions,
	pickerHelpConventions,
	pickerMenuControl,
	pickerNewMenuItem,
	pickerSettingValues,
} from './s9-picker-action-inventory.ts';
import type { PickerSetting } from './s9-picker-action-inventory.ts';
import { cellText, gridLines } from './terminal/terminal-grid.ts';
import type { TerminalGrid } from './terminal/terminal-grid.ts';
const text = (value: string): string => value.replace(/\s+/gu, ' ').trim();
const actionSignature = (action: ActionProjection): string =>
	JSON.stringify([text(action.name), [...action.keys].map(text).toSorted()]);
export function checkClosedActions(
	expected: readonly ActionProjection[],
	actual: readonly ActionProjection[],
): readonly string[] {
	const wanted = expected.map(actionSignature).toSorted();
	const observed = actual.map(actionSignature).toSorted();
	if (new Set(observed).size !== observed.length) return ['duplicate action projection'];
	return JSON.stringify(wanted) === JSON.stringify(observed)
		? []
		: ['missing, extra or changed action/key projection'];
}
export function checkProjectionParity(
	expected: readonly ActionProjection[],
	menu: readonly ActionProjection[],
	help: readonly ActionProjection[],
	toolbar: readonly ActionProjection[],
): readonly string[] {
	const failures = [...checkClosedActions(expected, menu), ...checkClosedActions(expected, help)];
	for (const action of toolbar)
		if (!expected.some((candidate) => actionSignature(candidate) === actionSignature(action)))
			failures.push('toolbar action differs from its menu/Help definition: ' + action.name);
	return failures;
}
export function checkPickerProjections(facts: {
	readonly expectedSettings: readonly ActionProjection[];
	readonly backgroundNew: ActionProjection | null;
	readonly menu: readonly ActionProjection[];
	readonly helpActionsAndSettings: readonly ActionProjection[];
	readonly helpConventions: readonly ActionProjection[];
	readonly helpMenuControl: ActionProjection;
	readonly conventionsHeading: boolean;
	readonly toolbar: readonly ActionProjection[];
}): readonly string[] {
	const expected = [
		...pickerBaseActions,
		...facts.expectedSettings,
		...(facts.backgroundNew === null ? [] : [facts.backgroundNew]),
	];
	const failures = [
		...checkProjectionParity(
			expected,
			facts.menu,
			facts.helpActionsAndSettings,
			facts.toolbar.filter((item) => item.name !== pickerMenuControl.name),
		),
		...checkClosedActions(
			[pickerMenuControl],
			facts.toolbar.filter((item) => item.name === pickerMenuControl.name),
		),
		...checkClosedActions(pickerHelpConventions, facts.helpConventions),
		...checkClosedActions([pickerMenuControl], [facts.helpMenuControl]),
	];
	if (!facts.conventionsHeading) failures.push('Help lacks its own input-conventions heading');
	if (facts.backgroundNew !== null && facts.backgroundNew.name !== pickerNewMenuItem)
		failures.push('background New has the wrong action name');
	return failures;
}
export interface PickerSettingProjection extends ActionProjection {
	readonly setting: PickerSetting;
	readonly value: string;
	readonly checked: boolean;
}
export function checkPickerSettings(
	actual: readonly PickerSettingProjection[],
	selected: Readonly<Record<PickerSetting, string>>,
): readonly string[] {
	const failures: string[] = [];
	for (const setting of ['folderScope', 'runtimeView', 'sort'] as const) {
		const expected = pickerSettingValues[setting];
		const rows = actual.filter((row) => row.setting === setting);
		if (
			JSON.stringify(rows.map((row) => row.value).toSorted()) !==
			JSON.stringify([...expected.values].toSorted())
		)
			failures.push('missing, extra or duplicate checked setting value: ' + setting);
		const checked = rows.filter((row) => row.checked);
		if (checked.length !== 1 || checked[0]?.value !== selected[setting])
			failures.push('checked setting differs from current value: ' + setting);
		for (const row of rows)
			if (
				JSON.stringify([...row.keys].toSorted()) !== JSON.stringify([...expected.keys].toSorted())
			)
				failures.push('setting key changed: ' + setting);
	}
	return failures;
}
export function checkRightDropoff(
	wideButtons: readonly string[],
	narrowButtons: readonly string[],
	menuActions: readonly string[],
): readonly string[] {
	const failures: string[] = [];
	if (JSON.stringify(narrowButtons) !== JSON.stringify(wideButtons.slice(0, narrowButtons.length)))
		failures.push('toolbar buttons did not drop from the right');
	for (const action of wideButtons)
		if (!menuActions.includes(action)) failures.push('toolbar action absent from menu: ' + action);
	return failures;
}
export function checkNoBottomActions(grid: TerminalGrid): readonly string[] {
	const bottom = gridLines(grid)
		.slice(-3)
		.filter((line) => !/^[\s│┃╭╮╰╯┌┐└┘─━]+$/u.test(line));
	const shortcuts = ['F1', 'F3', 'F4', 'F5', 'Ctrl+R', 'Ctrl+E', 'Ctrl+C'];
	const labels = ['Details', 'Filters', 'Pool', 'Options', 'Floor', 'Help', 'Quit'];
	return bottom.some(
		(line) =>
			shortcuts.filter((key) => line.includes(key)).length >= 2 ||
			labels.filter((label) => line.includes(label)).length >= 3,
	)
		? ['bottom shortcut/action row remains']
		: [];
}
export function checkToolbarOrder(
	grid: TerminalGrid,
	surface: 'sessions' | 'quota',
	options: {
		readonly countText: string;
		readonly contextText: string;
		readonly floorAvailable: boolean;
	},
): readonly string[] {
	const row = gridLines(grid).find((line) => line.includes('≡') && line.includes('Menu'));
	if (row === undefined) return ['top row has no ≡ Menu'];
	const failures: string[] = [];
	const required =
		surface === 'sessions'
			? ['≡', 'Menu', 'F10', '+ New', '/', 'Filter', 'This folder', 'All']
			: [
					'≡',
					'Menu',
					'F10',
					'Details',
					'Filters:',
					'Pool',
					'Options',
					...(options.floorAvailable ? ['Floor'] : []),
				];
	let position = -1;
	for (const token of required) {
		const next = row.indexOf(token, position + 1);
		if (next < 0) failures.push('missing/out-of-order toolbar item: ' + token);
		else position = next;
	}
	if (row.includes('Help') || row.includes('Quit'))
		failures.push('Help or Quit appears on toolbar');
	if (!row.includes(options.countText) || !row.includes(options.contextText))
		failures.push('right-hand count or machine/client context missing');
	for (const cells of grid.cells) {
		const menu = cells.find((cell) => cellText(cell) === '≡');
		if (menu !== undefined && menu.width !== 1) failures.push('menu icon is not one terminal cell');
	}
	return failures;
}
export const quotaCoreMenu: readonly ActionProjection[] = [
	{ name: 'Details', keys: ['F4'] },
	{ name: 'Filters…', keys: ['F3'] },
	{ name: 'Pool details', keys: ['F5'] },
	{ name: 'Options…', keys: ['Ctrl+R'] },
	{ name: 'Weekly floor…', keys: ['Ctrl+E'] },
	{ name: 'Help', keys: ['F1'] },
	{ name: 'Quit', keys: ['Ctrl+C'] },
];
export const accountActionNames = [
	'Log in again',
	'Options…',
	'Weekly floor…',
	'Disable account',
	'Copy account ID',
] as const;
export function checkAccountActions(
	actual: readonly ActionProjection[],
	disabled: boolean,
): readonly string[] {
	const expected: string[] = [...accountActionNames].map((name) =>
		name === 'Disable account' && disabled ? 'Enable account' : name,
	);
	if (disabled) expected.push('Remove account…');
	const names = actual.map((action) => action.name).toSorted();
	const failures =
		JSON.stringify(names) === JSON.stringify(expected.toSorted())
			? []
			: ['account Actions inventory differs from R17'];
	for (const action of actual) {
		if (action.keys.length === 0 || action.keys.some((key) => !key.trim()))
			failures.push('account action has no declared key: ' + action.name);
		if (action.name === 'Options…' && !action.keys.includes('Ctrl+R'))
			failures.push('Options key changed');
		if (action.name === 'Weekly floor…' && !action.keys.includes('Ctrl+E'))
			failures.push('Floor key changed');
	}
	return failures;
}
export function checkRemoveConfirmation(facts: {
	readonly primary: string;
	readonly focused: string;
	readonly removeFocused: boolean;
	readonly commandCalls: number;
}): readonly string[] {
	return facts.primary === 'Cancel' &&
		facts.focused === 'Cancel' &&
		!facts.removeFocused &&
		facts.commandCalls === 0
		? []
		: ['Remove confirmation must initially make Cancel primary/focused and dispatch nothing'];
}

/** Scope to Session details: underlying toolbar text cannot supply a missing Open. */
export function checkStoppedClaudeDetails(grid: TerminalGrid): readonly string[] {
	const dialog = findDialog(grid, 'Session details');
	if (dialog === null) return ['stopped Claude has no active Session details dialog'];
	const content = grid.cells
		.slice(dialog.top, dialog.bottom + 1)
		.map((row) =>
			row
				.slice(dialog.left, dialog.right + 1)
				.map(cellText)
				.join(''),
		)
		.join('\n');
	const failures: string[] = [];
	if (!/\bOpen\b/u.test(content)) failures.push('stopped Claude Details lacks Open');
	if (/Resume\s+in\s+background/iu.test(content))
		failures.push('stopped Claude Details exposes removed Resume in background');
	return failures;
}
