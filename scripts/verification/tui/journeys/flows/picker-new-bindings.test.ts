/** Literal grid controls qualify the oracle, never a product UI pass. */
import { expect, test } from 'vitest';

import { createTerminalCore, snapshotGrid } from '../../terminal/terminal-grid.ts';
import { checkNewMachineDropdown } from './picker-new-bindings.ts';
const line = (value: string): string => '│' + value.padEnd(38) + '│';

async function checkOptions(
	rows: readonly string[],
	source: 'local' | 'default' | 'configured',
): Promise<readonly string[]> {
	const core = await createTerminalCore(40, 9);
	try {
		core.writeRaw(
			Buffer.from(
				[
					'╭' + '─'.repeat(38) + '╮',
					line('Filter:'),
					...rows.map(line),
					'╰' + '─'.repeat(38) + '╯',
				].join('\r\n'),
			),
		);
		return checkNewMachineDropdown(snapshotGrid(core), source);
	} finally {
		core.dispose();
	}
}
const gold = '\x1b[38;2;215;186;125m';
const ordinary = '\x1b[39m';

test('LocalCodex accepts one committed highlighted machine and rejects a fabricated second choice', async () => {
	expect(await checkOptions([gold + '● Local Codex' + ordinary], 'local')).toEqual([]);
	expect(
		await checkOptions([gold + '● Local Codex' + ordinary, '○ Fabricated machine'], 'local'),
	).toContain('LocalCodex must expose exactly one Machine option');
});
test('uncommitted hosted highlight differs from the preserved committed Machine mark', async () => {
	expect(
		await checkOptions(['● This machine', gold + '○ Fixture remote' + ordinary], 'default'),
	).toEqual([]);
	expect(
		await checkOptions(['○ This machine', gold + '● Fixture remote' + ordinary], 'default'),
	).toContain('committed Machine mark changed before Start');
	expect(
		await checkOptions([gold + '● This machine' + ordinary, '○ Fixture remote'], 'default'),
	).toContain('declared highlighted Machine is not visibly focused');
});
test('configured source keeps its committed remote while highlighting the default without picking', async () => {
	expect(
		await checkOptions([gold + '○ This machine' + ordinary, '● Fixture remote'], 'configured'),
	).toEqual([]);
	expect(
		await checkOptions([gold + '● This machine' + ordinary, '○ Fixture remote'], 'configured'),
	).toContain('committed Machine mark changed before Start');
});
