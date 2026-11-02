import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { realpathSync, statSync } from 'node:fs';

import { expect, test } from 'vitest';

import type { ProcessIdentity } from '../evidence/evidence-schema.ts';
import {
	lookupProcessIdentity,
	requireOwnedProductChild,
	sameIdentity,
} from './process-identity.ts';

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

test('product child binding requires the exact executable and one direct owned process', async () => {
	const children: ReturnType<typeof spawn>[] = [];
	const start = async (): Promise<void> => {
		const child = spawn(
			process.execPath,
			['-e', "process.on('message',()=>process.disconnect());process.send('ready');"],
			{ stdio: ['ignore', 'ignore', 'ignore', 'ipc'] },
		);
		children.push(child);
		await once(child, 'message');
	};
	try {
		await start();
		const child = children[0];
		if (child?.pid === undefined) throw new Error('controlled child identity missing');
		const pid = child.pid;
		const product = realpathSync(process.execPath);
		expect(requireOwnedProductChild(process.pid, product).pid).toBe(child.pid);
		expect(() => requireOwnedProductChild(process.pid, '/not-the-declared-product')).toThrow(
			'one exact owned',
		);
		expect(() => requireOwnedProductChild(pid, product)).toThrow('one exact owned');
		await start();
		expect(() => requireOwnedProductChild(process.pid, product)).toThrow('one exact owned');
	} finally {
		await Promise.all(
			children.map(async (child) => {
				const closed = once(child, 'close');
				child.send('release');
				await closed;
				expect(child.exitCode).toBe(0);
			}),
		);
	}
});
