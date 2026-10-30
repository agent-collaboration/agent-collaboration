import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { statSync } from 'node:fs';

import { expect, test } from 'vitest';

import type { ProcessIdentity } from './evidence-schema.ts';
import { lookupProcessIdentity, sameIdentity } from './process-identity.ts';

// macOS installs /bin/ps setuid root: a user-launched ps runs with effective UID 0 and the
// launching account's real UID. Hosts without that split cannot exercise this case through ps.
const setuidSystemPs = ((): boolean => {
	const status = statSync('/bin/ps');
	return status.uid === 0 && (status.mode & 0o4000) !== 0;
})();

test.skipIf(!setuidSystemPs)(
	'a user-launched setuid system utility keeps its launching account as real owner',
	async () => {
		// Repeated comm columns exceed what the pipe holds, so the actual setuid ps stays blocked in
		// write until drained; only backpressure matters here, not other processes' arguments.
		const held = spawn(
			'/bin/ps',
			['-axww', ...Array.from({ length: 40 }, () => ['-o', 'comm=']).flat()],
			{ stdio: ['ignore', 'pipe', 'ignore'] },
		);
		await once(held.stdout, 'data');
		held.stdout.pause();
		try {
			if (held.pid === undefined) throw new Error('held setuid utility PID unavailable');
			const identity = lookupProcessIdentity(held.pid);
			expect(identity?.realUid).toBe(process.getuid?.());
			expect(identity?.effectiveUid).toBe(0);
		} finally {
			held.stdout.resume();
			await once(held, 'close');
		}
		expect(held.exitCode).toBe(0);
	},
);

test('identity ignores an effective-only transition but not a real owner or reuse change', () => {
	const recorded: ProcessIdentity = {
		pid: 40,
		realUid: 501,
		effectiveUid: 501,
		group: 10,
		birth: 'Mon Oct 26 10:20:30 2026',
		executable: '/owned/provider',
	};
	expect(sameIdentity({ ...recorded, effectiveUid: 0 }, recorded)).toBe(true);
	for (const changed of [
		{ realUid: 502 },
		{ birth: 'Tue Oct 27 10:20:30 2026' },
		{ group: 99 },
		{ executable: '/other/executable' },
	])
		expect(sameIdentity({ ...recorded, ...changed }, recorded)).toBe(false);
	expect(sameIdentity(null, recorded)).toBe(false);
});
