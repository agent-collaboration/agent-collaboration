/** Bounded Ghostty-compatible key subset, derived from kitty spec and v1.3.1 tests.
 * This is not libghostty's encoder: @wterm/ghostty 0.5.7 exports no such API.
 */
import type { MouseEncoding } from '@wterm/core';
import type { GhosttyCore } from '@wterm/ghostty';
import { z } from 'zod';

export interface TerminalInputModes {
	readonly kittyFlags: number;
	readonly cursorKeysApplication: boolean;
	readonly mouseTracking: 0 | 1000 | 1002 | 1003;
	readonly mouseEncoding: MouseEncoding | null;
	readonly columns: number;
	readonly rows: number;
}
export function readGhosttyInputModes(core: GhosttyCore): TerminalInputModes {
	return {
		kittyFlags: core.kittyKeyboardFlags(),
		cursorKeysApplication: core.cursorKeysApp(),
		mouseTracking: core.mouseTracking(),
		mouseEncoding: core.mouseEncoding(),
		columns: core.getCols(),
		rows: core.getRows(),
	};
}
export const ghosttyKeySchema = z.enum([
	'Enter',
	'Ctrl+Enter',
	'Ctrl+Shift+Enter',
	'Shift+Enter',
	'Ctrl+C',
	'Ctrl+D',
	'Ctrl+E',
	'Ctrl+F',
	'Ctrl+G',
	'Ctrl+N',
	'Ctrl+R',
	'Escape',
	'F1',
	'F2',
	'F3',
	'F4',
	'F5',
	'F10',
	'Tab',
	'Shift+Tab',
	'Up',
	'Down',
	'Alt+Up',
	'Alt+Down',
	'Alt+Shift+Up',
	'Alt+Shift+Down',
	'Left',
	'Right',
	'PageUp',
	'PageDown',
	'Home',
	'End',
]);
export type GhosttyKey = z.infer<typeof ghosttyKeySchema>;
interface KeyEntry {
	readonly code: number;
	readonly final: string;
	readonly legacy: string;
	readonly modifier: number;
	readonly cursor?: string;
}
const entry = (code: number, final: string, legacy: string, modifier = 1): KeyEntry => ({
	code,
	final,
	legacy,
	modifier,
});
const keyEntries: Readonly<Record<GhosttyKey, KeyEntry>> = {
	Enter: entry(13, 'u', '\r'),
	'Ctrl+Enter': entry(13, 'u', '\x1b[27;5;13~', 5),
	'Ctrl+Shift+Enter': entry(13, 'u', '\x1b[27;6;13~', 6),
	'Shift+Enter': entry(13, 'u', '\x1b[27;2;13~', 2),
	'Ctrl+C': entry(99, 'u', '\x03', 5),
	'Ctrl+D': entry(100, 'u', '\x04', 5),
	'Ctrl+E': entry(101, 'u', '\x05', 5),
	'Ctrl+F': entry(102, 'u', '\x06', 5),
	'Ctrl+G': entry(103, 'u', '\x07', 5),
	'Ctrl+N': entry(110, 'u', '\x0e', 5),
	'Ctrl+R': entry(114, 'u', '\x12', 5),
	Escape: entry(27, 'u', '\x1b'),
	Tab: entry(9, 'u', '\t'),
	'Shift+Tab': entry(9, 'u', '\x1b[Z', 2),
	['F1']: entry(1, 'P', '\x1bOP'),
	['F2']: entry(1, 'Q', '\x1bOQ'),
	['F3']: entry(13, '~', '\x1bOR'),
	['F4']: entry(1, 'S', '\x1bOS'),
	['F5']: entry(15, '~', '\x1b[15~'),
	F10: entry(21, '~', '\x1b[21~'),
	['Up']: { ...entry(1, 'A', '\x1b[A'), cursor: 'A' },
	Down: { ...entry(1, 'B', '\x1b[B'), cursor: 'B' },
	'Alt+Up': entry(1, 'A', '\x1b[1;3A', 3),
	'Alt+Down': entry(1, 'B', '\x1b[1;3B', 3),
	'Alt+Shift+Up': entry(1, 'A', '\x1b[1;4A', 4),
	'Alt+Shift+Down': entry(1, 'B', '\x1b[1;4B', 4),
	Left: { ...entry(1, 'D', '\x1b[D'), cursor: 'D' },
	Right: { ...entry(1, 'C', '\x1b[C'), cursor: 'C' },
	PageUp: entry(5, '~', '\x1b[5~'),
	PageDown: entry(6, '~', '\x1b[6~'),
	Home: { ...entry(1, 'H', '\x1b[H'), cursor: 'H' },
	End: { ...entry(1, 'F', '\x1b[F'), cursor: 'F' },
};
/** Press events only. Hardware/layout, locks, IME and arbitrary modifiers are outside this subset. */
export function encodeGhosttyKey(key: GhosttyKey, modes: TerminalInputModes): Buffer {
	const current = keyEntries[key];
	if (modes.kittyFlags === 0)
		return Buffer.from(
			current.cursor !== undefined && modes.cursorKeysApplication
				? '\x1bO' + current.cursor
				: current.legacy,
		);
	if ((key === 'Enter' || key === 'Tab') && (modes.kittyFlags & 8) === 0)
		return Buffer.from(current.legacy);
	const special = current.final !== 'u' && current.final !== '~';
	if (special && (modes.kittyFlags & 2) !== 0)
		return Buffer.from(`\x1b[1;${current.modifier}:1${current.final}`);
	const modifier = current.modifier === 1 ? '' : ';' + current.modifier;
	const code = special && current.modifier === 1 ? '' : String(current.code);
	return Buffer.from(`\x1b[${code}${modifier}${current.final}`);
}
export function encodeGhosttyText(text: string, modes: TerminalInputModes): Buffer {
	const sequences: string[] = [];
	// The protocol names Unicode code points; this intentionally is not grapheme segmentation.
	for (const character of text) {
		const code = character.codePointAt(0);
		if (code === undefined || code < 32 || code === 127)
			throw new Error('text input is printable text, not a key or paste');
		sequences.push(`\x1b[${code}${(modes.kittyFlags & 16) === 0 ? '' : ';;' + code}u`);
	}
	return Buffer.from((modes.kittyFlags & 8) === 0 ? text : sequences.join(''));
}
export type CellMouseEvent =
	| {
			readonly kind: 'press' | 'release';
			readonly button: 'left' | 'middle' | 'right';
			readonly column: number;
			readonly row: number;
	  }
	| {
			readonly kind: 'wheel';
			readonly direction: 'up' | 'down';
			readonly column: number;
			readonly row: number;
	  };
export type MouseDelivery =
	| { readonly kind: 'bytes'; readonly bytes: Buffer }
	| { readonly kind: 'consumed'; readonly reason: 'mouse-reporting-disabled' }
	| { readonly kind: 'unsupported'; readonly reason: string };
export function encodeGhosttyMouse(
	event: CellMouseEvent,
	modes: TerminalInputModes,
): MouseDelivery {
	if (
		!Number.isInteger(event.column) ||
		!Number.isInteger(event.row) ||
		event.column < 0 ||
		event.row < 0 ||
		event.column >= modes.columns ||
		event.row >= modes.rows
	)
		throw new Error('mouse coordinate outside current viewport');
	if (modes.mouseTracking === 0) return { kind: 'consumed', reason: 'mouse-reporting-disabled' };
	if (modes.mouseEncoding !== 'sgr')
		return {
			kind: 'unsupported',
			reason: `cell SGR1006 required; observed ${modes.mouseEncoding ?? 'unknown'}`,
		};
	const code =
		event.kind === 'wheel'
			? event.direction === 'up'
				? 64
				: 65
			: { left: 0, middle: 1, right: 2 }[event.button];
	return {
		kind: 'bytes',
		bytes: Buffer.from(
			`\x1b[<${code};${event.column + 1};${event.row + 1}${event.kind === 'release' ? 'm' : 'M'}`,
		),
	};
}
