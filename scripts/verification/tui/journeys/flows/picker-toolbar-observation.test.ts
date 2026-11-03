/** Literal cells qualify reader sensitivity; these scenes are not product or native B evidence. */
import { expect, test } from 'vitest';

import { createTerminalCore, snapshotGrid } from '../../terminal/terminal-grid.ts';
import { readPickerMenuViewport } from './picker-menu-observation.ts';
import {
	checkCurrentPickerToolbar,
	pickerFilterFocused,
	pickerVisibleFocus,
} from './picker-toolbar-observation.ts';

type TestCore = Awaited<ReturnType<typeof createTerminalCore>>;
function paintToolbarCells(core: TestCore, column: number, content: string): void {
	core.writeString(`\x1b[2;${column + 1}H${content}`);
}
async function toolbarCore(width: 32 | 120): Promise<TestCore> {
	const core = await createTerminalCore(width, 6);
	core.writeString('╭' + '─'.repeat(width - 2) + '╮');
	core.writeString('\x1b[6;1H╰' + '─'.repeat(width - 2) + '╯');
	paintToolbarCells(core, 0, '│');
	paintToolbarCells(core, width - 1, '│');
	paintToolbarCells(core, 2, '≡ Menu F10');
	if (width === 120) {
		paintToolbarCells(core, 16, '+ New Ctrl+N');
		paintToolbarCells(core, 32, '/ Filter…');
		paintToolbarCells(core, 60, 'This folder');
		paintToolbarCells(core, 74, 'Repo');
		paintToolbarCells(core, 81, 'All');
		paintToolbarCells(core, 91, '6/6 sessions');
		paintToolbarCells(core, 104, '·');
		paintToolbarCells(core, 106, 'This machine');
	} else {
		paintToolbarCells(core, 15, '/ Filter…');
		paintToolbarCells(core, 27, '6/6');
	}
	return core;
}

test.each([32, 120] as const)(
	'the %s-column count stays visible and at the right edge',
	async (width) => {
		const core = await toolbarCore(width);
		try {
			expect(checkCurrentPickerToolbar(snapshotGrid(core))).toEqual([]);
			paintToolbarCells(core, width === 32 ? 27 : 91, ' '.repeat(width === 32 ? 3 : 12));
			expect(checkCurrentPickerToolbar(snapshotGrid(core))).toContain(
				'declared 6/6 count disappeared or moved before Filter',
			);
		} finally {
			core.dispose();
		}
	},
);

test('narrow Filter keeps eleven outer cells and rejects a dropped or undersized field', async () => {
	const core = await toolbarCore(32);
	try {
		paintToolbarCells(core, 27, '   ');
		paintToolbarCells(core, 26, '6/6 ');
		expect(checkCurrentPickerToolbar(snapshotGrid(core))).toContain(
			'32-column Filter has fewer than eleven outer cells',
		);
		paintToolbarCells(core, 15, '          ');
		expect(checkCurrentPickerToolbar(snapshotGrid(core))).toContain(
			'Filter disappeared from the toolbar',
		);
	} finally {
		core.dispose();
	}
});

test('scope drops before New, and visible focus excludes an unpainted target', async () => {
	const core = await toolbarCore(120);
	try {
		paintToolbarCells(core, 16, ' '.repeat(12));
		expect(checkCurrentPickerToolbar(snapshotGrid(core))).toContain(
			'New dropped before the scope switch',
		);
		expect(pickerVisibleFocus(snapshotGrid(core))).toBe(false);
		paintToolbarCells(core, 81, '\x1b[4;38;2;215;186;125mAll\x1b[0m');
		expect(pickerVisibleFocus(snapshotGrid(core))).toBe(true);
		expect(pickerFilterFocused(snapshotGrid(core))).toBe(false);
		paintToolbarCells(core, 81, 'All');
		paintToolbarCells(core, 34, '\x1b[4;38;2;215;186;125m \x1b[0m');
		expect(pickerFilterFocused(snapshotGrid(core))).toBe(true);
	} finally {
		core.dispose();
	}
});

test('the physical menu reader keeps item keys/checks and rejects extra actions or focusable headings', async () => {
	const core = await createTerminalCore(50, 12);
	const paint = (extra: string): void => {
		core.writeString(
			'\x1b[2J\x1b[H' +
				[
					'╭' + '─'.repeat(48) + '╮',
					...[
						'❯ New session… Ctrl+N',
						'Filter /',
						'Scope Ctrl+S',
						'This folder',
						'Repo',
						'✓ All',
						extra,
					].map((row) => '│' + row.padEnd(48) + '│'),
					'╰' + '─'.repeat(48) + '╯',
				].join('\r\n'),
		);
	};
	try {
		paint('');
		const observed = readPickerMenuViewport(snapshotGrid(core));
		expect(observed.items).toEqual([
			{ title: 'New session…', key: 'Ctrl+N', checked: false },
			{ title: 'Filter', key: '/', checked: false },
			{ title: 'This folder', key: '', checked: false },
			{ title: 'Repo', key: '', checked: false },
			{ title: 'All', key: '', checked: true },
		]);
		expect(observed.headings).toEqual(['Scope']);
		paint('Unexpected action');
		expect(() => readPickerMenuViewport(snapshotGrid(core))).toThrow('unexpected painted');
		paint('❯ Group by F6');
		expect(() => readPickerMenuViewport(snapshotGrid(core))).toThrow('heading became focusable');
	} finally {
		core.dispose();
	}
});
