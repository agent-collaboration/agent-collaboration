/** Qualify the pinned Node libghostty VT core; no product behaviour is inferred.
 * node scripts/verification/tui/terminal-qualification.ts OUTPUT.json
 */
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { z } from 'zod';

import { isMain, reportCliFailure } from './cli-arguments.ts';
import { createTerminalCore, ghosttyWasmBytes, gridCell, snapshotGrid } from './terminal-grid.ts';
import type { TerminalGrid } from './terminal-grid.ts';

export interface EmulatorQualification {
	readonly kind: 'wterm-ghostty-qualification';
	readonly versions: Readonly<Record<string, string>>;
	readonly wtermRelease: string;
	readonly libghosttySource: string;
	readonly wasmSha256: string;
	readonly node: string;
	readonly checks: readonly string[];
	readonly grid: TerminalGrid;
}

export async function qualifyTerminal(): Promise<EmulatorQualification> {
	const checks: string[] = [];
	const versions = Object.fromEntries(
		['core', 'ghostty', 'dom'].map((name) => {
			const root = dirname(dirname(fileURLToPath(import.meta.resolve(`@wterm/${name}`))));
			const metadata = z
				.object({ version: z.literal('0.5.7') })
				.parse(JSON.parse(readFileSync(join(root, 'package.json'), 'utf8')));
			return [`@wterm/${name}`, metadata.version];
		}),
	);
	const core = await createTerminalCore(16, 8);
	try {
		assert.equal('document' in globalThis, false);
		checks.push('public GhosttyCore loads in Node with no DOM');
		// Fixed host defaults make named colours independently decidable.
		core.setThemeColors({
			foreground: 0xeeeeee,
			background: 0x111111,
			cursor: 0xffffff,
			palette: [
				0x000000, 0x800000, 0x008000, 0x808000, 0x000080, 0x800080, 0x008080, 0xc0c0c0, 0x808080,
				0xff0000, 0x00ff00, 0xffff00, 0x0000ff, 0xff00ff, 0x00ffff, 0xffffff,
			],
		});
		core.writeRaw(Buffer.from('\x1b[31;44;1;2mN\x1b[0m\x1b[38;2;18;52;86;48;2;101;67;33mT'));
		let grid = snapshotGrid(core);
		assert.equal(gridCell(grid, 0, 0).fgRgb, 0x800000);
		assert.equal(gridCell(grid, 0, 0).bgRgb, 0x000080);
		assert.equal(gridCell(grid, 0, 0).flags & 3, 3);
		assert.equal(gridCell(grid, 0, 1).fgRgb, 0x123456);
		assert.equal(gridCell(grid, 0, 1).bgRgb, 0x654321);
		checks.push('named colours, truecolour foreground/background, bold and dim');
		for (const [index, glyph] of ['📂', '✉️', '界'].entries()) {
			const row = index + 1;
			core.writeString(`\x1b[0m\x1b[${row + 1};1H${glyph}M`);
			grid = snapshotGrid(core);
			const cell = gridCell(grid, row, 0);
			assert.equal(cell.chars ?? String.fromCodePoint(cell.char), glyph);
			assert.equal(cell.width, 2);
			assert.equal(gridCell(grid, row, 1).width, 0);
			assert.equal(gridCell(grid, row, 2).char, 77);
			// The glyph starts at overlay-left−1; a real gutter write must
			// clear it while preserving the border at overlay-left.
			core.writeString(`\x1b[${row + 1};1H\x1b[48;2;30;40;50m ┌──┐`);
			grid = snapshotGrid(core);
			assert.equal(gridCell(grid, row, 0).char, 32);
			assert.equal(gridCell(grid, row, 1).char, '┌'.codePointAt(0));
			assert.equal(gridCell(grid, row, 0).bgRgb, 0x1e2832);
		}
		checks.push('📂, ✉️ and 界 preserve two-cell widths/graphemes and overlay-left−1 gutter');
		core.writeString('\x1b[0m\x1b[5;16H界');
		grid = snapshotGrid(core);
		assert.equal(gridCell(grid, 4, 15).spacerHead, true);
		assert.equal(gridCell(grid, 5, 0).width, 2);
		assert.deepEqual(grid.rowMetadata[4], { wrapsToNext: true, continuesPrevious: false });
		assert.deepEqual(grid.rowMetadata[5], { wrapsToNext: false, continuesPrevious: true });
		checks.push('wide right-edge spacerHead and native soft-wrap metadata');
		core.writeString('\x1b[?u');
		assert.equal(core.getResponse(), '\x1b[?0u');
		core.writeString('\x1b[>1u\x1b[?u');
		assert.equal(core.kittyKeyboardFlags(), 1);
		assert.equal(core.getResponse(), '\x1b[?1u');
		assert.equal(core.getResponse(), null);
		checks.push('kitty CSI ? u replies from the engine before/after negotiation');
		// The Codex 0.165 startup probe (tui/src/terminal_probe.rs:277-296) asks for the cursor
		// position, primary device attributes and default colours; DA2 only over SSH.
		core.writeString('\x1b[0m\x1b[3;5H\x1b[6n');
		assert.equal(core.getResponse(), '\x1b[3;5R');
		core.writeString('\x1b[c');
		assert.equal(core.getResponse(), '\x1b[?1;2c');
		core.writeString('\x1b]10;?\x1b\\\x1b]11;?\x1b\\');
		assert.equal(core.getResponse(), '\x1b]10;rgb:eeee/eeee/eeee\x1b\\');
		assert.equal(core.getResponse(), '\x1b]11;rgb:1111/1111/1111\x1b\\');
		core.writeString('\x1b[>c');
		assert.equal(core.getResponse(), null);
		checks.push('DSR 6n, DA1 and OSC 10/11 replies come from the engine; DA2 is unanswered');
		core.resize(12, 6);
		core.writeString('\x1b[2J\x1b[6;12HZ');
		grid = snapshotGrid(core);
		assert.equal(grid.columns, 12);
		assert.equal(grid.rows, 6);
		assert.equal(gridCell(grid, 5, 11).char, 90);
		assert.equal(grid.cursor.row, 5);
		checks.push('same-core resize, final cell and cursor readback');
		return {
			kind: 'wterm-ghostty-qualification',
			versions,
			wtermRelease: 'f2016031d6381d90ed90bfbe1862a9ee1127acb0',
			libghosttySource: 'ghostty-org/ghostty v1.3.1; wterm release patches/build layer',
			wasmSha256: createHash('sha256').update(ghosttyWasmBytes()).digest('hex'),
			node: process.version,
			checks,
			grid,
		};
	} finally {
		core.dispose();
	}
}

if (isMain(import.meta.url)) {
	const output = process.argv[2];
	if (output === undefined) throw new Error('usage: terminal-qualification.ts OUTPUT.json');
	await qualifyTerminal()
		.then((receipt) => {
			writeFileSync(output, `${JSON.stringify(receipt, null, 2)}\n`);
			process.stdout.write(`qualified ${receipt.checks.length} checks; ${output}\n`);
		})
		.catch(reportCliFailure);
}
