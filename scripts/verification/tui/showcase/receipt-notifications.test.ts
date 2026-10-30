import { spawn } from 'node:child_process';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { describe, expect, it } from 'vitest';

import { ReceiptNotifications } from './receipt-notifications.ts';

describe('retained receipt notifications', () => {
	it('observes repeated writes while the child keeps both descriptors open', async () => {
		const directory = mkdtempSync(join(tmpdir(), 'showcase-notifications-'));
		const notifications = new ReceiptNotifications(directory);
		const child = spawn(
			process.execPath,
			[
				'--input-type=module',
				'-e',
				`
import {openSync,writeSync} from 'node:fs';
import {join} from 'node:path';
const descriptors=['input-dispatch.jsonl','showcase-state.jsonl'].map(name=>openSync(join(process.argv[1],name),'wx'));
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
	});
	it('does not accept preexisting state without a real notification witness', async () => {
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
