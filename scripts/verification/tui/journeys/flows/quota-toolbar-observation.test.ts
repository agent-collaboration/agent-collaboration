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
	expect(checkQuotaHelpRows(literalHelp.flatMap((row) => [row.slice(0, 8), row.slice(8)]))).toEqual(
		[],
	);
});
test.each([
	['missing action', literalHelp.filter((row) => row !== 'R Refresh')],
	['extra action', literalHelp.map((row) => (row === 'R Refresh' ? 'R Refresh Export' : row))],
	['wrong key', literalHelp.map((row) => (row === 'F5 Pool' ? 'F8 Pool' : row))],
	['wrong convention', literalHelp.map((row) => row.replace('stays on quota', 'quits quota'))],
])('Help rejects %s', (_label, rows) => {
	expect(checkQuotaHelpRows(rows).length).toBeGreaterThan(0);
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
