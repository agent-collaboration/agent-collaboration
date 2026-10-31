import { expect, test } from 'vitest';

import {
	appendOneHelpRow,
	helpViewport,
	helpLayout,
	requireHelpNoop,
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
		// UiGlyph::Current (●) marks a choice; focused ListRow paints
		// UiGlyph::Focus (❯), as defined by the shipped theme.rs mapping.
		expect(() => selectedFixtureRow(snapshotGrid(core), records)).toThrow('missing or ambiguous');
		core.writeString('\x1b[4;1H│ ❯ Codex Alpha                │');
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
		expect(helpLayout(snapshotGrid(core), ['First', 'Second', 'Third', 'Fourth']).kind).toBe(
			'overflow',
		);
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
test('fitting Help requires its entire census and rejects a marked-frame mutation', async () => {
	const core = await createTerminalCore(40, 12);
	try {
		core.writeString(
			'\x1b[2;2H╭────────────────────╮\x1b[3;2H│Help                │\x1b[4;2H│Context             │\x1b[5;2H│Screen Demo         │\x1b[6;2H│Input conventions   │\x1b[7;2H│Alt Move            │\x1b[8;2H│                    │\x1b[9;2H│Close Help          │\x1b[10;2H╰────────────────────╯',
		);
		const census = ['Context', 'Screen Demo', 'Input conventions', 'Alt Move'];
		const before = snapshotGrid(core);
		expect(helpLayout(before, census).kind).toBe('fit');
		core.writeString('\x1b[?2026h\x1b[?2026l');
		requireHelpNoop(before, snapshotGrid(core));
		core.writeString('\x1b[7;3HX');
		expect(() => requireHelpNoop(before, snapshotGrid(core))).toThrow('changed its marked frame');
		expect(helpLayout(snapshotGrid(core), census).kind).toBe('incomplete');
		core.writeString('\x1b[7;3H                    ');
		const incomplete = helpLayout(snapshotGrid(core), census);
		expect(incomplete.kind).toBe('incomplete');
		if (incomplete.kind !== 'incomplete') throw new Error('clipped Help was accepted as fitting');
		expect(incomplete.reason).toContain('Alt Move');
	} finally {
		core.dispose();
	}
});
