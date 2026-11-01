import { expect, test } from 'vitest';

import { createTerminalCore, snapshotGrid } from '../../terminal/terminal-grid.ts';
import type { TerminalGrid } from '../../terminal/terminal-grid.ts';
import { sceneMarkerContracts } from './grid-scene-contract.ts';
import { findDialog, gridFieldPair, observeGridScene } from './grid-scene-matcher.ts';

async function screen(bytes: string): Promise<TerminalGrid> {
	const core = await createTerminalCore(120, 36);
	try {
		core.writeString(bytes);
		return snapshotGrid(core);
	} finally {
		core.dispose();
	}
}
test('all 138 ported scenes reject an actual empty libghostty grid', async () => {
	const blank = await screen('');
	expect(Object.keys(sceneMarkerContracts)).toHaveLength(138);
	for (const checkpoint of Object.keys(sceneMarkerContracts))
		expect(observeGridScene(blank, checkpoint).passed, checkpoint).toBe(false);
});
test('wrong scene and wrong count cannot satisfy a main screen checkpoint', async () => {
	const good = 'Sessions Help This machine All providers 6/6 Codex Alpha';
	expect(observeGridScene(await screen(good), 'compact-sessions-all').passed).toBe(true);
	expect(
		observeGridScene(await screen(good.replace('6/6', '6/60')), 'compact-sessions-all').passed,
	).toBe(false);
	expect(
		observeGridScene(
			await screen('Quota Help Account filters Move field Close'),
			'compact-sessions-all',
		).passed,
	).toBe(false);
});
const paint = (value: string): string =>
	`\x1b[1;1HClient Codex\x1b[3;10H┌─────────────────────────────────────────────┐\x1b[4;10H│ Session filters Help                       │\x1b[5;10H│ Client ${value}                                    │\x1b[6;10H│ Apply Cancel                               │\x1b[7;10H└─────────────────────────────────────────────┘`;
test('background field value never supplies a missing active-dialog value', async () => {
	const wrong = await screen(paint('Cursor'));
	const observed = observeGridScene(wrong, 'compact-sessions-chooser');
	expect(observed.dialog).not.toBeNull();
	expect(observed.missing).toContain('Client Codex');
	expect(observeGridScene(await screen(paint('Codex ▾')), 'compact-sessions-chooser').passed).toBe(
		true,
	);
});
test('dialog detection does not borrow a title from a later nested frame', async () => {
	const grid = await screen(
		'\x1b[1;1H┌──────────────────────────────────────────────────────────┐\x1b[6;1H└──────────────────────────────────────────────────────────┘\x1b[4;10H┌──────────────────────────────────┐\x1b[6;10H│ Account details                  │\x1b[9;10H└──────────────────────────────────┘',
	);
	expect(findDialog(grid, 'Account details')).toEqual({ left: 9, top: 3, right: 44, bottom: 8 });
});

test('duplicate conflicting values for one dialog field are rejected', async () => {
	const grid = await screen(
		paint('Codex ▾')
			.replace('\x1b[6;10H', '\x1b[6;10H│ Client Cursor ▾\x1b[8;10H')
			.replace('\x1b[7;10H', '\x1b[9;10H'),
	);
	expect(observeGridScene(grid, 'compact-sessions-chooser').passed).toBe(false);
});

test('a stacked committed Machine accepts its focus arrow and rejects an uncommitted value', async () => {
	const machine = (value: string): string =>
		[
			'╭──────────────────────────╮',
			'│New session               │',
			'│Machine                   │',
			`│❯ ${value.padEnd(23)} │`,
			'╰──────────────────────────╯',
		].join('\r\n');
	expect(
		gridFieldPair(
			await screen(machine('Fixture remote ▾')),
			'New session',
			'Machine',
			'Fixture remote',
		),
	).toBe(true);
	expect(
		gridFieldPair(
			await screen(machine('This machine ▾')),
			'New session',
			'Machine',
			'Fixture remote',
		),
	).toBe(false);
});
