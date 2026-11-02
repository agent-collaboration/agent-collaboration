/** Reader sensitivity over literal display content; no native/product evidence. */
import { expect, test } from 'vitest';

import { createTerminalCore, snapshotGrid } from '../../terminal/terminal-grid.ts';
import { checkQuotaHelpRows, checkQuotaToolbar } from './quota-toolbar-observation.ts';
const literalHelp = [
	'Context',
	'Pool Example pool',
	'Actions',
	'F10 Menu',
	'F4, Enter Details',
	'F3, Ctrl+F Filters…',
	'F5 Pool',
	'Ctrl+R Options',
	'Ctrl+E Floor',
	'Log in again',
	'R Refresh',
	'F6 Group by',
	'None',
	'Provider',
	'Status',
	'F1, Ctrl+7, Ctrl+_, Ctrl+/ Help',
	'Q, Ctrl+C, Ctrl+D Quit',
	'Input conventions',
	'↑ / ↓ Select an account',
	'Tab/Shift+Tab Move between controls',
	'Esc Close the top menu or dialog; stays on quota',
	'Ctrl+C Exit Quota unless a reset is being applied.',
];
test('Help keeps the complete closed action set and conventions across physical wrapping', () => {
	expect(
		checkQuotaHelpRows(
			literalHelp.flatMap((row) => [row.slice(0, 8), row.slice(8)]),
			{ refreshAvailable: true, weeklyFloorAvailable: true },
		),
	).toEqual([]);
});
test.each([
	['missing action', literalHelp.filter((row) => row !== 'R Refresh')],
	['extra action', literalHelp.map((row) => (row === 'R Refresh' ? 'R Refresh Export' : row))],
	['wrong key', literalHelp.map((row) => (row === 'F5 Pool' ? 'F8 Pool' : row))],
	['wrong convention', literalHelp.map((row) => row.replace('stays on quota', 'quits quota'))],
])('Help rejects %s', (_label, rows) => {
	expect(
		checkQuotaHelpRows(rows, { refreshAvailable: true, weeklyFloorAvailable: true }).length,
	).toBeGreaterThan(0);
});
test.each([
	['Details Filters: All ▾ Pool Options Floor', true],
	['Details Filters: All ▾ Options Floor', false],
	['Details Pool Filters: All ▾ Options Floor', false],
	['Details Filters: All ▾ Pool Options Floor Help', false],
])('wide toolbar observes literal prefix and order: %s', async (buttons, valid) => {
	const core = await createTerminalCore(120, 36);
	try {
		core.writeString('\r\n ≡ Menu F10 ' + buttons + ' Quota · 2/2 · 1 client');
		expect(checkQuotaToolbar(snapshotGrid(core)).length === 0).toBe(valid);
	} finally {
		core.dispose();
	}
});

const unavailableRefreshHelp = literalHelp.map((row) =>
	row === 'R Refresh' ? 'R Refresh Quota refresh is unavailable' : row,
);
test('read-only Help requires the exact unavailable Refresh reason across physical wrapping', () => {
	expect(
		checkQuotaHelpRows(
			unavailableRefreshHelp.flatMap((row) => [row.slice(0, 13), row.slice(13)]),
			{ refreshAvailable: false, weeklyFloorAvailable: true },
		),
	).toEqual([]);
});
test.each([
	['missing reason', literalHelp, false],
	['reason on available refresh', unavailableRefreshHelp, true],
	[
		'wrong reason',
		unavailableRefreshHelp.map((row) =>
			row.replace('Quota refresh is unavailable', 'Quota refresh failed'),
		),
		false,
	],
	[
		'unknown extra reason',
		unavailableRefreshHelp.map((row) => row.replace('F5 Pool', 'F5 Pool Unavailable')),
		false,
	],
	[
		'extra action after reason',
		unavailableRefreshHelp.map((row) =>
			row.replace('Quota refresh is unavailable', 'Quota refresh is unavailable Export'),
		),
		false,
	],
] as const)('Help rejects capability mismatch: %s', (_name, rows, refreshAvailable) => {
	expect(
		checkQuotaHelpRows(rows, { refreshAvailable, weeklyFloorAvailable: true }).length,
	).toBeGreaterThan(0);
});

// Floor is a declared fixture capability, independent of Refresh.
test('Help without a weekly-floor saver accepts the closed set without Floor', () => {
	expect(
		checkQuotaHelpRows(
			literalHelp.filter((row) => row !== 'Ctrl+E Floor'),
			{ refreshAvailable: true, weeklyFloorAvailable: false },
		),
	).toEqual([]);
});
test('Help rejects a missing Floor action when the saver exists', () => {
	expect(
		checkQuotaHelpRows(
			literalHelp.filter((row) => row !== 'Ctrl+E Floor'),
			{ refreshAvailable: true, weeklyFloorAvailable: true },
		).length,
	).toBeGreaterThan(0);
});
test('Help rejects an unexpected Floor action when the saver is absent', () => {
	expect(
		checkQuotaHelpRows(literalHelp, { refreshAvailable: true, weeklyFloorAvailable: false }).length,
	).toBeGreaterThan(0);
});
