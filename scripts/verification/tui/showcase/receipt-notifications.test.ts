import { spawn } from 'node:child_process';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { describe, expect, it } from 'vitest';

import { ReceiptNotifications } from './receipt-notifications.ts';

describe('retained receipt notifications', () => {
	it.each([false, true])(
		'observes repeated open-writer writes (already-created pair=%s)',
		async (existing) => {
			const directory = mkdtempSync(join(tmpdir(), 'showcase-notifications-'));
			if (existing)
				for (const name of ['input-dispatch.jsonl', 'showcase-state.jsonl'])
					writeFileSync(join(directory, name), '');
			const notifications = new ReceiptNotifications(directory);
			const child = spawn(
				process.execPath,
				[
					'--input-type=module',
					'-e',
					`
import {openSync,writeSync} from 'node:fs';
import {join} from 'node:path';
const descriptors=['input-dispatch.jsonl','showcase-state.jsonl'].map(name=>openSync(join(process.argv[1],name),${existing ? "'a'" : "'wx'"}));
for(const descriptor of descriptors) writeSync(descriptor,'row\\n');
process.on('message',()=>{for(const descriptor of descriptors) writeSync(descriptor,'row\\n');});
`,
					directory,
				],
				{ stdio: ['ignore', 'ignore', 'inherit', 'ipc'] },
			);
			try {
				await notifications.ready();
				await [2, 3, 4].reduce<Promise<void>>(async (previous, count) => {
					await previous;
					await notifications.wait({
						description: 'open writer row ' + count,
						inject: async () => {
							child.send('append');
						},
						matches: () =>
							['input-dispatch.jsonl', 'showcase-state.jsonl'].every(
								(name) =>
									readFileSync(join(directory, name), 'utf8').trim().split('\n').length === count,
							)
								? true
								: null,
					});
				}, Promise.resolve());
				expect(
					notifications.history().filter((row) => row.kind === 'file-write').length,
				).toBeGreaterThan(0);
			} finally {
				notifications.close();
				const exited = new Promise<void>((resolve) => child.once('exit', () => resolve()));
				child.kill('SIGTERM');
				await exited;
			}
		},
	);
	it('does not accept an incomplete receipt pair as ready', async () => {
		const directory = mkdtempSync(join(tmpdir(), 'showcase-no-witness-'));
		writeFileSync(join(directory, 'input-dispatch.jsonl'), 'retained\n');
		const notifications = new ReceiptNotifications(directory);
		const pending = notifications.wait({
			description: 'unwitnessed content',
			matches: () => true,
			inject: async () => {},
		});
		const rejected = expect(pending).rejects.toThrow('closed');
		notifications.close();
		await rejected;
	});
});
