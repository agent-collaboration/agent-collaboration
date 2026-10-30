import { expect, test } from 'vitest';

import { createTerminalCore, gridCell, gridLines, snapshotGrid } from './terminal-grid.ts';
import { qualifyTerminal } from './terminal-qualification.ts';

test('libghostty qualifies against independent grids, colours, Unicode, protocol and resize', async () => {
	const receipt = await qualifyTerminal();
	expect(receipt.checks).toHaveLength(6);
	expect(receipt.wasmSha256).toMatch(/^[a-f0-9]{64}$/u);
});

test('PTY byte chunks preserve partial UTF8 and CSI state; snapshots are immutable', async () => {
	const core = await createTerminalCore(12, 4);
	try {
		const bytes = Buffer.from('\x1b[38;2;1;2;3m📂界');
		for (const byte of bytes) core.writeRaw(Uint8Array.of(byte));
		const before = snapshotGrid(core);
		expect(gridLines(before)[0]).toBe('📂界        ');
		expect(gridCell(before, 0, 0).fgRgb).toBe(0x010203);
		core.writeString('\x1b[2J\x1b[Hblank');
		expect(gridLines(before)[0]).toContain('📂界');
		expect(gridLines(snapshotGrid(core))[0]).toContain('blank');
	} finally {
		core.dispose();
	}
});

test('a combined startup chunk has two independent engine responses to drain', async () => {
	const core = await createTerminalCore(12, 4);
	try {
		core.writeString('\x1b[?u\x1b[c');
		expect(core.getResponse()).toBe('\x1b[?0u');
		expect(core.getResponse()).toBe('\x1b[?1;2c');
		expect(core.getResponse()).toBe(null);
	} finally {
		core.dispose();
	}
});
