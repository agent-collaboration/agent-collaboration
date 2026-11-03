/** Source-contract reader controls over literal emulator bytes; no product UI proof. */
import { expect, test } from 'vitest';

import { createTerminalCore, snapshotGrid } from '../../terminal/terminal-grid.ts';
import { locatePaintedTarget } from './painted-flow-target.ts';
import {
	quotaDialogTarget,
	quotaDialogInteriorRows,
	quotaExpectedKey,
	quotaOverlay,
	quotaPrimary,
	quotaScreen,
} from './quota-flow-inputs.ts';

test('actual white button text is distinct from the grey duplicate key hint in the active dialog', async () => {
	const core = await createTerminalCore(76, 30);
	try {
		core.writeString(
			'╭──────────────────────────────────────╮\r\n│ Account details                      │\r\n│                                      │\r\n│ \x1b[97mLog in again\x1b[0m                         │\r\n│ \x1b[90mCtrl+Enter Log in again\x1b[0m              │\r\n╰──────────────────────────────────────╯',
		);
		const grid = snapshotGrid(core);
		expect(
			locatePaintedTarget(grid, quotaDialogTarget(grid, 'Account details', 'Log in again')),
		).toEqual({ column: 2, row: 3, width: 12 });
	} finally {
		core.dispose();
	}
});
test('primary marker is required in the named active dialog', async () => {
	const core = await createTerminalCore(76, 30);
	try {
		core.writeString(
			'╭──────────────────────────────────────╮\r\n│ Account details                      │\r\n│ ● Log in again                        │\r\n│ Close                                │\r\n╰──────────────────────────────────────╯',
		);
		const grid = snapshotGrid(core);
		expect(quotaPrimary(grid, 'Account details', 'Log in again')).toBe(true);
		expect(quotaPrimary(grid, 'Account details', 'Close')).toBe(false);
		expect(quotaPrimary(grid, 'Weekly floor', 'Log in again')).toBe(false);
	} finally {
		core.dispose();
	}
});
test('source event encodings preserve uppercase text modifiers, aliases, Back Tab and global Quit owner', () => {
	expect(quotaExpectedKey('R', quotaScreen, null, 'quota:refresh')).toEqual({
		event: { kind: 'key', code: { kind: 'text' }, modifiers: ['shift'], phase: 'press' },
		surface: { kind: 'screen' },
		target: null,
		action: 'quota:refresh',
	});
	expect(quotaExpectedKey('Alt+Shift+Down', quotaScreen, 'action:quota:accounts')).toMatchObject({
		event: { code: { kind: 'fixed', name: 'Down' }, modifiers: ['shift', 'alt'] },
	});
	expect(quotaExpectedKey('Shift+Tab', quotaOverlay(1))).toMatchObject({
		event: { code: { kind: 'fixed', name: 'Back Tab' }, modifiers: ['shift'] },
		surface: { kind: 'overlay', overlayId: '1' },
	});
	expect(quotaExpectedKey('Ctrl+C', quotaScreen, null, 'quota:quit')).toEqual({
		event: {
			kind: 'key',
			code: { kind: 'shortcut', key: 'c' },
			modifiers: ['control'],
			phase: 'press',
		},
		surface: { kind: 'screen' },
		target: null,
		action: 'quota:quit',
	});
});

test('fitting dialog rows exclude covered screen text outside the active border', async () => {
	const core = await createTerminalCore(76, 30);
	try {
		core.writeString(
			'cover ╭────────────────────────╮ suffix\r\ncover │ Account details        │ suffix\r\ncover │ Body                   │ suffix\r\ncover ╰────────────────────────╯ suffix',
		);
		const rows = quotaDialogInteriorRows(snapshotGrid(core), 'Account details');
		expect(rows.join(' ')).toContain('Body');
		expect(rows.join(' ')).not.toContain('cover');
		expect(rows.join(' ')).not.toContain('suffix');
	} finally {
		core.dispose();
	}
});
