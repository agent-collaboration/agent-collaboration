/** Terminal key encodings for journey input: legacy bytes, or kitty keyboard-protocol sequences. */
import type { KeyProtocol } from '../terminal/pty-grid-driver.ts';

export function encodeJourneyKey(key: string, protocol: KeyProtocol): Buffer {
	const legacy: Readonly<Record<string, string>> = {
		Enter: '\r',
		Escape: '\x1b',
		Tab: '\t',
		'Shift+Tab': '\x1b[Z',
		['Up']: '\x1b[A',
		Down: '\x1b[B',
		Right: '\x1b[C',
		PageUp: '\x1b[5~',
		PageDown: '\x1b[6~',
		['F1']: '\x1bOP',
		['F4']: '\x1bOS',
		['F5']: '\x1b[15~',
	};
	const kitty: Readonly<Record<string, string>> = {
		Enter: '\x1b[13u',
		Escape: '\x1b[27u',
		Tab: '\x1b[9u',
		'Shift+Tab': '\x1b[9;2u',
		['Up']: '\x1b[1;1A',
		Down: '\x1b[1;1B',
		Right: '\x1b[1;1C',
		PageUp: '\x1b[5;1~',
		PageDown: '\x1b[6;1~',
		['F1']: '\x1b[1;1P',
		['F4']: '\x1b[1;1S',
		['F5']: '\x1b[15;1~',
	};
	if (/^Ctrl\+[A-Z]$/u.test(key)) {
		const code = key.toLowerCase().charCodeAt(key.length - 1);
		return Buffer.from(protocol === 'kitty' ? `\x1b[${code};5u` : String.fromCodePoint(code - 96));
	}
	const sequence = (protocol === 'kitty' ? kitty : legacy)[key];
	if (sequence === undefined) throw new Error(`unsupported journey key: ${key}`);
	return Buffer.from(sequence);
}
