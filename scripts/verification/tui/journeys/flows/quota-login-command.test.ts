/** Exact painted-command controls; actual child argv remains separately verified. */
import { expect, test } from 'vitest';

import { quotaLoginCommandMatches } from './quota-login-bindings.ts';
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
