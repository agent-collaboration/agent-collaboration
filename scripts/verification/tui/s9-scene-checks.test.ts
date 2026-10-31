import { expect, test } from 'vitest';

import {
	pickerBaseActions,
	pickerHelpConventions,
	pickerMenuControl,
} from './s9-picker-action-inventory.ts';
import {
	checkAccountActions,
	checkClosedActions,
	checkNoBottomActions,
	checkProjectionParity,
	checkRemoveConfirmation,
	checkRightDropoff,
	checkToolbarOrder,
	checkStoppedClaudeDetails,
	checkPickerProjections,
	checkPickerSettings,
} from './s9-scene-checks.ts';
import { createTerminalCore, snapshotGrid } from './terminal-grid.ts';
import type { TerminalGrid } from './terminal-grid.ts';
async function screen(bytes: string): Promise<TerminalGrid> {
	const core = await createTerminalCore(120, 36);
	try {
		core.writeString(bytes);
		return snapshotGrid(core);
	} finally {
		core.dispose();
	}
}
test('an empty main body does not turn its top toolbar into a bottom footer', async () => {
	const top = '≡ Menu F10 Details Filters: All ▾ Pool Options Floor Accounts 2 Clients 1';
	expect(checkNoBottomActions(await screen(top))).toEqual([]);
	expect(
		checkNoBottomActions(await screen(top + '\x1b[35;1HF4 Details F3 Filters F5 Pool F1 Help')),
	).not.toEqual([]);
});
const stoppedClaudeDialog = (action: string): string =>
	'Open in underlying toolbar\x1b[3;4H╭──────────────────────────────────╮\x1b[4;4H│ Session details                  │\x1b[5;4H│ ' +
	action +
	'\x1b[5;39H│\x1b[6;4H╰──────────────────────────────────╯';
test('stopped Claude Details offers Open and rejects the removed background resume', async () => {
	expect(checkStoppedClaudeDetails(await screen(stoppedClaudeDialog('Open')))).toEqual([]);
	expect(checkStoppedClaudeDetails(await screen(stoppedClaudeDialog('Close')))).toContain(
		'stopped Claude Details lacks Open',
	);
	expect(
		checkStoppedClaudeDetails(await screen(stoppedClaudeDialog('Open Resume in background'))),
	).toContain('stopped Claude Details exposes removed Resume in background');
	expect(checkStoppedClaudeDetails(await screen('Open'))).not.toEqual([]);
});
test('closed action sets reject omissions additions duplicate rows and wrong keys', () => {
	const expected = [
		{ name: 'Details', keys: ['Enter', 'F4'] },
		{ name: 'Help', keys: ['F1'] },
	];
	expect(
		checkClosedActions(expected, [
			expected[1] ?? { name: '', keys: [] },
			expected[0] ?? { name: '', keys: [] },
		]),
	).toEqual([]);
	expect(checkClosedActions(expected, expected.slice(1))).not.toEqual([]);
	expect(
		checkClosedActions(expected, [...expected, { name: 'Invented', keys: ['F2'] }]),
	).not.toEqual([]);
	expect(
		checkClosedActions(expected, [...expected, expected[0] ?? { name: '', keys: [] }]),
	).not.toEqual([]);
	expect(
		checkClosedActions(expected, [
			{ name: 'Details', keys: ['Ctrl+Enter'] },
			{ name: 'Help', keys: ['F1'] },
		]),
	).not.toEqual([]);
	expect(
		checkProjectionParity(expected, expected, expected, [{ name: 'Help', keys: ['F2'] }]),
	).not.toEqual([]);
});
test('settled picker projections keep conventions in Help and Menu F10 outside its own menu', () => {
	const expectedSettings = [
		{ name: 'This folder', keys: ['Ctrl+S'] },
		{ name: 'All', keys: ['Ctrl+S'] },
	];
	const menu = [...pickerBaseActions, ...expectedSettings];
	const good = {
		expectedSettings,
		backgroundNew: null,
		menu,
		helpActionsAndSettings: menu,
		helpConventions: pickerHelpConventions,
		helpMenuControl: pickerMenuControl,
		conventionsHeading: true,
		toolbar: [pickerMenuControl],
	};
	expect(checkPickerProjections(good)).toEqual([]);
	expect(checkPickerProjections({ ...good, menu: [...menu, pickerMenuControl] })).not.toEqual([]);
	expect(
		checkPickerProjections({
			...good,
			menu: [...menu, { name: 'Move selection', keys: ['↑', '↓'] }],
		}),
	).not.toEqual([]);
	expect(
		checkPickerProjections({ ...good, helpConventions: pickerHelpConventions.slice(1) }),
	).not.toEqual([]);
	expect(checkPickerProjections({ ...good, conventionsHeading: false })).not.toEqual([]);
	expect(
		checkPickerProjections({ ...good, helpMenuControl: { name: 'Menu', keys: ['F2'] } }),
	).not.toEqual([]);
});
test('checked settings retain every value and key with exactly the current value checked', () => {
	const rows = [
		{
			setting: 'folderScope',
			value: 'This folder',
			name: 'This folder',
			keys: ['Ctrl+S'],
			checked: true,
		},
		{ setting: 'folderScope', value: 'All', name: 'All', keys: ['Ctrl+S'], checked: false },
		{ setting: 'runtimeView', value: 'All', name: 'Runtime All', keys: ['Ctrl+T'], checked: true },
		{
			setting: 'runtimeView',
			value: 'Blocked',
			name: 'Runtime Blocked',
			keys: ['Ctrl+T'],
			checked: false,
		},
		{
			setting: 'runtimeView',
			value: 'Active',
			name: 'Runtime Active',
			keys: ['Ctrl+T'],
			checked: false,
		},
		{
			setting: 'runtimeView',
			value: 'Idle',
			name: 'Runtime Idle',
			keys: ['Ctrl+T'],
			checked: false,
		},
		{ setting: 'sort', value: 'updated', name: 'Sort updated', keys: ['Ctrl+O'], checked: true },
		{ setting: 'sort', value: 'created', name: 'Sort created', keys: ['Ctrl+O'], checked: false },
	] satisfies Parameters<typeof checkPickerSettings>[0];
	const selected = { folderScope: 'This folder', runtimeView: 'All', sort: 'updated' };
	expect(checkPickerSettings(rows, selected)).toEqual([]);
	expect(checkPickerSettings(rows.slice(1), selected)).not.toEqual([]);
	expect(
		checkPickerSettings(
			rows.map((row) => ({ ...row, checked: true })),
			selected,
		),
	).not.toEqual([]);
	expect(
		checkPickerSettings(
			rows.map((row) => ({ ...row, keys: ['Ctrl+X'] })),
			selected,
		),
	).not.toEqual([]);
	expect(checkPickerSettings(rows, { ...selected, sort: 'created' })).not.toEqual([]);
});
test('narrow toolbar is a prefix and every lost button stays in the menu', () => {
	const wide = ['Details', 'Filters', 'Pool', 'Options', 'Floor'];
	expect(checkRightDropoff(wide, ['Details', 'Filters'], wide)).toEqual([]);
	expect(checkRightDropoff(wide, ['Details', 'Pool'], wide)).not.toEqual([]);
	expect(checkRightDropoff(wide, ['Details'], wide.slice(0, 2))).not.toEqual([]);
});
test('wide toolbar preserves order context and one-cell menu glyph without Help or Quit', async () => {
	const options = { countText: '6/6', contextText: 'This machine', floorAvailable: false };
	const good = '≡ Menu F10 + New / Filter… This folder | All 6/6 This machine';
	expect(checkToolbarOrder(await screen(good), 'sessions', options)).toEqual([]);
	expect(checkToolbarOrder(await screen(good.replace('≡', '☰')), 'sessions', options)).not.toEqual(
		[],
	);
	expect(checkToolbarOrder(await screen(good + ' Help'), 'sessions', options)).not.toEqual([]);
	expect(
		checkToolbarOrder(await screen(good.replace('6/6', '0/6')), 'sessions', options),
	).not.toEqual([]);
});
test('enabled Actions omit Remove and disabled Actions replace Disable with Enable', () => {
	const enabled = [
		{ name: 'Log in again', keys: ['declared-login'] },
		{ name: 'Options…', keys: ['Ctrl+R'] },
		{ name: 'Weekly floor…', keys: ['Ctrl+E'] },
		{ name: 'Disable account', keys: ['declared-disable'] },
		{ name: 'Copy account ID', keys: ['declared-copy'] },
	];
	expect(checkAccountActions(enabled, false)).toEqual([]);
	expect(
		checkAccountActions(
			[...enabled, { name: 'Remove account…', keys: ['declared-remove'] }],
			false,
		),
	).not.toEqual([]);
	const disabled = enabled.map((action) =>
		action.name === 'Disable account'
			? { name: 'Enable account', keys: ['declared-enable'] }
			: action,
	);
	disabled.push({ name: 'Remove account…', keys: ['declared-remove'] });
	expect(checkAccountActions(disabled, true)).toEqual([]);
	expect(
		checkAccountActions(
			disabled.map((action) => ({ name: action.name, keys: [] })),
			true,
		),
	).not.toEqual([]);
});
test('Remove begins on primary Cancel with zero dispatched commands', () => {
	const good = { primary: 'Cancel', focused: 'Cancel', removeFocused: false, commandCalls: 0 };
	expect(checkRemoveConfirmation(good)).toEqual([]);
	expect(checkRemoveConfirmation({ ...good, primary: 'Remove' })).not.toEqual([]);
	expect(checkRemoveConfirmation({ ...good, focused: 'Remove', removeFocused: true })).not.toEqual(
		[],
	);
	expect(checkRemoveConfirmation({ ...good, commandCalls: 1 })).not.toEqual([]);
});
