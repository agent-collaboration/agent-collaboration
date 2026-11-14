/** Exact painted-command controls; actual child argv remains separately verified. */
import { expect, test } from 'vitest';

import { createTerminalCore, snapshotGrid } from '../../terminal/terminal-grid.ts';
import { quotaLoginCommandMatches, quotaLoginPrimaryMatches } from './quota-login-bindings.ts';
const command = 'codex-router account login --provider openai --label effect-alpha';
test('the exact captured command survives physical wrapping', () => {
	expect(
		quotaLoginCommandMatches([
			'Login',
			command.slice(0, 20),
			command.slice(20),
			'Quota windows',
			'weekly ?%',
		]),
	).toBe(true);
});
test.each([
	['extra root', [command + ' --router-root /other', 'Quota windows']],
	['extra provider', [command + ' --provider claude', 'Quota windows']],
	['label suffix', [command + '-other', 'Quota windows']],
	['missing section', [command]],
	['duplicate command', [command, command, 'Quota windows']],
])('painted login command rejects %s', (_name, rows) => {
	expect(quotaLoginCommandMatches(rows)).toBe(false);
});

test.each([
	['Required', 'Log in again'],
	['SignedOut', 'Log in again'],
	['SignedIn', 'Close'],
	['Unknown', 'Close'],
	['Retrying', 'Close'],
	['InProgress', 'Close'],
] as const)('login state %s requires the exact painted primary %s', async (state, primary) => {
	const core = await createTerminalCore(76, 30);
	try {
		for (const painted of ['Log in again', 'Close']) {
			core.writeString(
				'\x1b[2J\x1b[H' +
					'╭──────────────────────────────────────╮\r\n│ Account details                      │\r\n│ ● ' +
					painted.padEnd(35) +
					'│\r\n│ Log in again   Close                  │\r\n╰──────────────────────────────────────╯',
			);
			expect(quotaLoginPrimaryMatches(snapshotGrid(core), state)).toBe(painted === primary);
		}
	} finally {
		core.dispose();
	}
});
