import { expect, test } from 'vitest';

import { encodeGhosttyKey } from './ghostty-input-encoding.ts';
import type { GhosttyKey, TerminalInputModes } from './ghostty-input-encoding.ts';
import { createTerminalCore } from './terminal-grid.ts';

const legacy: TerminalInputModes = {
	kittyFlags: 0,
	cursorKeysApplication: false,
	mouseTracking: 1000,
	mouseEncoding: 'sgr',
	columns: 120,
	rows: 36,
};
test('independent libghostty literals distinguish flag1 from report-all', () => {
	// Ghostty v1.3.1 src/input/key_encode.zig:1238-1263,1336-1376,1627-1650.
	for (const [key, flags, bytes] of [
		['Enter', 1, '\r'],
		['Tab', 1, '\t'],
		['Shift+Enter', 1, '\x1b[13;2u'],
		['Shift+Tab', 1, '\x1b[9;2u'],
		['Up', 1, '\x1b[A'],
		['Enter', 31, '\x1b[13u'],
	] satisfies readonly (readonly [GhosttyKey, number, string])[]) {
		expect(encodeGhosttyKey(key, { ...legacy, kittyFlags: flags }).toString()).toBe(bytes);
	}
});
test('layer C keys encode under actual flags rather than a boolean kitty label', () => {
	const literals = [
		['Ctrl+Enter', '\x1b[27;5;13~', '\x1b[13;5u'],
		['Ctrl+Shift+Enter', '\x1b[27;6;13~', '\x1b[13;6u'],
		['Ctrl+C', '\x03', '\x1b[99;5u'],
		['Escape', '\x1b', '\x1b[27u'],
		['F10', '\x1b[21~', '\x1b[21~'],
		['Tab', '\t', '\t'],
		['Shift+Enter', '\x1b[27;2;13~', '\x1b[13;2u'],
		['Up', '\x1b[A', '\x1b[A'],
		['Down', '\x1b[B', '\x1b[B'],
		['Left', '\x1b[D', '\x1b[D'],
		['Right', '\x1b[C', '\x1b[C'],
		['PageUp', '\x1b[5~', '\x1b[5~'],
		['PageDown', '\x1b[6~', '\x1b[6~'],
		['Home', '\x1b[H', '\x1b[H'],
		['End', '\x1b[F', '\x1b[F'],
	] satisfies readonly (readonly [GhosttyKey, string, string])[];
	for (const [key, oldBytes, enhancedBytes] of literals) {
		expect(encodeGhosttyKey(key, legacy).toString()).toBe(oldBytes);
		expect(encodeGhosttyKey(key, { ...legacy, kittyFlags: 1 }).toString()).toBe(enhancedBytes);
	}
	expect(encodeGhosttyKey('Up', { ...legacy, cursorKeysApplication: true }).toString()).toBe(
		'\x1bOA',
	);
	expect(encodeGhosttyKey('Up', { ...legacy, kittyFlags: 3 }).toString()).toBe('\x1b[1;1:1A');
});
test('pinned VT core tracks real keyboard stack, mouse modes and resized dimensions', async () => {
	const core = await createTerminalCore(120, 36);
	try {
		core.writeString('\x1b[>1u\x1b[?1002h\x1b[?1006h\x1b[?1h');
		expect(core.kittyKeyboardFlags()).toBe(1);
		expect(core.mouseTracking()).toBe(1002);
		expect(core.mouseEncoding()).toBe('sgr');
		expect(core.cursorKeysApp()).toBe(true);
		core.writeString('\x1b[>31u');
		expect(core.kittyKeyboardFlags()).toBe(31);
		core.writeString('\x1b[<u');
		expect(core.kittyKeyboardFlags()).toBe(1);
		core.resize(32, 24);
		expect([core.getCols(), core.getRows()]).toEqual([32, 24]);
		core.writeString('\x1b[<u\x1b[?1002l\x1b[?1006l');
		expect(core.kittyKeyboardFlags()).toBe(0);
		expect(core.mouseTracking()).toBe(0);
	} finally {
		core.dispose();
	}
});
