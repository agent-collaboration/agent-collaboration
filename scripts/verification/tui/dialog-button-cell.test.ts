/** Button-role oracle over controlled VT data, not product behavior. */
import { expect, test } from 'vitest';

import { detailsActionCell, targetCell } from './input-journeys.ts';
import { createTerminalCore, snapshotGrid } from './terminal-grid.ts';
import type { TerminalGrid } from './terminal-grid.ts';
async function drawnDialog(
	buttonStyle: string,
	hintStyle: string,
	secondButton = false,
): Promise<TerminalGrid> {
	const core = await createTerminalCore(120, 36);
	try {
		core.writeString(
			`\x1b[2;2H┌──────────────────────────────────────────────────────┐\x1b[3;2H│ New session                                          │\x1b[5;2H│ ${buttonStyle}● Start\x1b[0m\x1b[6;2H│ ${hintStyle}Ctrl+Enter Start\x1b[0m\x1b[7;2H│ ${secondButton ? buttonStyle + '● Start' : ''}\x1b[0m\x1b[8;2H└──────────────────────────────────────────────────────┘`,
		);
		return snapshotGrid(core);
	} finally {
		core.dispose();
	}
}
test.each(['\x1b[38;2;156;188;251m', '\x1b[93m'])(
	'button role %s disambiguates the generated Start hint',
	async (style) => {
		expect(targetCell(await drawnDialog(style, '\x1b[37m'), 'New session', 'Start')).toEqual({
			column: 5,
			row: 4,
		});
	},
);
test('hint-only or duplicate real button roles cannot supply a click target', async () => {
	const hintOnly = await drawnDialog('', '\x1b[37m');
	const duplicate = await drawnDialog('\x1b[93m', '\x1b[37m', true);
	expect(() => targetCell(hintOnly, 'New session', 'Start')).toThrow();
	expect(() => targetCell(duplicate, 'New session', 'Start')).toThrow();
});

test.each([
	['codex', 'Resume'],
	['native-claude', 'Open'],
] as const)('Details resume uses the %s row action %s', async (rowKind, label) => {
	const core = await createTerminalCore(120, 36);
	try {
		core.writeString(
			`\x1b[2;2H┌──────────────────────────────────────────────────────┐\x1b[3;2H│ Session details                                      │\x1b[5;2H│ \x1b[93m● ${label}\x1b[0m\x1b[8;2H└──────────────────────────────────────────────────────┘`,
		);
		const grid = snapshotGrid(core);
		expect(detailsActionCell(grid, rowKind, 'resume')).toEqual({ column: 5, row: 4 });
		expect(() =>
			detailsActionCell(grid, rowKind === 'codex' ? 'native-claude' : 'codex', 'resume'),
		).toThrow();
		expect(() => detailsActionCell(grid, rowKind, 'fork')).toThrow();
	} finally {
		core.dispose();
	}
});
