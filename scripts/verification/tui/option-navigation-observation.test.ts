import { expect, test } from 'vitest';

import {
	appendOneHelpRow,
	helpViewport,
	pickerPageRows,
	requireHelpRows,
	selectedFixtureRow,
} from './option-navigation-observation.ts';
import { createTerminalCore, snapshotGrid } from './terminal-grid.ts';

test('list oracle measures physical body rows and refuses a highlighted toolbar word as selection', async () => {
	const core = await createTerminalCore(48, 12);
	try {
		core.writeString(
			'\x1b[1;1H● All\x1b[2;1H┌──────────────────────────────┐\x1b[3;1H│Session                       │\x1b[4;1H│ ● Codex Alpha                │\x1b[10;1H└──────────────────────────────┘',
		);
		const records = [
			{ label: 'Alpha', visiblePrefix: 'Codex A' },
			{ label: 'Beta', visiblePrefix: 'Codex B' },
		];
		expect(selectedFixtureRow(snapshotGrid(core), records)).toBe(0);
		expect(pickerPageRows(snapshotGrid(core))).toBe(2);
		expect(pickerPageRows({ ...snapshotGrid(core), columns: 32 })).toBe(1);
		core.writeString('\x1b[4;1H│   Codex Alpha                │');
		expect(() => selectedFixtureRow(snapshotGrid(core), records)).toThrow('missing or ambiguous');
	} finally {
		core.dispose();
	}
});
test('Help oracle excludes chrome and rejects a one-row alias masquerading as a whole page', async () => {
	const core = await createTerminalCore(40, 12);
	try {
		core.writeString(
			'\x1b[2;2H╭──────────────────╮\x1b[3;2H│Help              │\x1b[4;2H│First            █│\x1b[5;2H│Second           ││\x1b[6;2H│Third            ││\x1b[7;2H│Close Help        │\x1b[8;2H╰──────────────────╯',
		);
		const observed = helpViewport(snapshotGrid(core));
		expect(observed.rows).toEqual(['First', 'Second', 'Third']);
		expect(observed.atEnd).toBe(false);
		expect(appendOneHelpRow(observed.rows, ['Second', 'Third', 'Fourth'])).toBe('Fourth');
		expect(() => appendOneHelpRow(observed.rows, ['Third', 'Fourth', 'Fifth'])).toThrow(
			'exactly one',
		);
		expect(() =>
			requireHelpRows(['Second', 'Third', 'Fourth'], ['Fourth', 'Fifth', 'Sixth']),
		).toThrow('physical page');
		requireHelpRows(['Fourth', 'Fifth', 'Sixth'], ['Fourth', 'Fifth', 'Sixth']);
	} finally {
		core.dispose();
	}
});
