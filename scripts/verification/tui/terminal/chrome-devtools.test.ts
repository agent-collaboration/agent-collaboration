import { spawn } from 'node:child_process';
import { readFileSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { expect, test } from 'vitest';
import { z } from 'zod';

import type { ProcessIdentity } from '../evidence/evidence-schema.ts';
import { controlledExecutable } from '../fixtures/controlled-child-fixtures.ts';
import { temporaryRoot } from '../fixtures/receipt-fixtures.ts';
import { lookupProcessIdentity, sameIdentity } from '../processes/process-identity.ts';

const browserReceiptSchema = z.strictObject({
	browser: z.number().int(),
	helper: z.number().int(),
});

test('original capture closes its actual HTTP listener even when browser teardown is denied', async () => {
	const root = temporaryRoot();
	const browser = controlledExecutable(
		root,
		'browser-with-close-denial',
		`
const fs=require('node:fs');const path=require('node:path');
const profile=process.argv.find(value=>value.startsWith('--user-data-dir=')).slice('--user-data-dir='.length);
fs.writeFileSync(path.join(path.dirname(profile),'browser-ready.json'),JSON.stringify({browser:process.pid}));
const input=fs.createReadStream(null,{fd:3});let buffer='';
input.on('data',bytes=>{buffer+=bytes.toString();while(buffer.includes('\\0')){
 const end=buffer.indexOf('\\0');const command=JSON.parse(buffer.slice(0,end));buffer=buffer.slice(end+1);
 fs.writeSync(4,JSON.stringify({id:command.id,result:{}})+'\\0');
 if(command.method==='Browser.close')process.exit(0);
}});
`,
	);
	const fixture = fileURLToPath(
		new URL('../fixtures/chrome-lifecycle-fixture.ts', import.meta.url),
	);
	const runner = spawn(process.execPath, [fixture, browser, root, 'close-error'], {
		stdio: ['ignore', 'ignore', 'pipe'],
	});
	let stderr = '';
	runner.stderr.on('data', (bytes: Buffer) => {
		stderr += bytes.toString();
	});
	const completion = new Promise<{ readonly code: number | null; readonly signal: string | null }>(
		(resolve, reject) => {
			runner.once('error', reject);
			runner.once('close', (code, signal) => resolve({ code, signal }));
		},
	);
	const identity = runner.pid === undefined ? null : lookupProcessIdentity(runner.pid);
	let guard: ReturnType<typeof setTimeout> | undefined;
	try {
		const result = await Promise.race([
			completion,
			new Promise<never>((_resolve, reject) => {
				guard = setTimeout(
					() => reject(new Error('asset listener held runner after close denial: ' + stderr)),
					5000,
				);
			}),
		]);
		expect(result).toEqual({ code: 1, signal: null });
		expect(stderr).toContain('forced group kill EPERM');
		const receipt = z
			.strictObject({ browser: z.number().int() })
			.parse(JSON.parse(readFileSync(join(root, 'browser-ready.json'), 'utf8')));
		expect(lookupProcessIdentity(receipt.browser)).toBeNull();
	} finally {
		clearTimeout(guard);
		if (identity !== null && sameIdentity(lookupProcessIdentity(identity.pid), identity))
			process.kill(identity.pid, 'SIGKILL');
		await completion;
		rmSync(root, { recursive: true, force: true });
	}
});

test.each(['cooperative', 'ignores-term'])(
	'forced final error exits naturally with zero browser/helper processes (%s)',
	async (helperMode) => {
		// Arrange: the controlled browser exits on Browser.close but its real helper keeps stderr open.
		const root = temporaryRoot();
		const browser = controlledExecutable(
			root,
			'browser-with-held-helper',
			`
const fs=require('node:fs');const {spawn}=require('node:child_process');
const helper=spawn(process.execPath,['-e',"process.send('ready');process.on('SIGTERM',()=>${helperMode === 'cooperative' ? 'process.exit(0)' : '{}'});setInterval(()=>{},1000);"],{stdio:['pipe','ignore',2,'ipc']});
helper.once('message',()=>{
 const input=fs.createReadStream(null,{fd:3});let buffer='';
 input.on('data',bytes=>{buffer+=bytes.toString();while(buffer.includes('\\0')){
  const end=buffer.indexOf('\\0');const command=JSON.parse(buffer.slice(0,end));buffer=buffer.slice(end+1);
  fs.writeSync(4,JSON.stringify({id:command.id,result:{browser:process.pid,helper:helper.pid}})+'\\0');
  if(command.method==='Browser.close')process.exit(0);
 }});
});
`,
		);
		const fixture = fileURLToPath(
			new URL('../fixtures/chrome-lifecycle-fixture.ts', import.meta.url),
		);
		const runner = spawn(process.execPath, [fixture, browser, root], {
			stdio: ['ignore', 'pipe', 'pipe'],
		});
		let stdout = '',
			stderr = '';
		runner.stdout.on('data', (bytes: Buffer) => {
			stdout += bytes.toString();
		});
		runner.stderr.on('data', (bytes: Buffer) => {
			stderr += bytes.toString();
		});
		const completion = new Promise<{
			readonly code: number | null;
			readonly signal: string | null;
		}>((resolve, reject) => {
			runner.once('error', reject);
			runner.once('close', (code, signal) => resolve({ code, signal }));
		});
		const runnerIdentity = runner.pid === undefined ? null : lookupProcessIdentity(runner.pid);
		const owned: ProcessIdentity[] = [];
		let guard: ReturnType<typeof setTimeout> | undefined;
		try {
			// Act: only the failure guard may stop a broken runner; correct cleanup must beat it.
			const result = await Promise.race([
				completion,
				new Promise<never>((_resolve, reject) => {
					guard = setTimeout(
						() => reject(new Error(`runner failed to exit; ${stdout}; ${stderr}`)),
						5000,
					);
				}),
			]);
			// Assert: actual exit and independent OS identities, not close() being called.
			expect(result).toEqual({ code: 1, signal: null });
			expect(stderr).toContain('forced final observation error');
			const receipt = browserReceiptSchema.parse(
				JSON.parse(readFileSync(join(root, 'browser-ready.json'), 'utf8')),
			);
			expect(lookupProcessIdentity(receipt.browser)).toBeNull();
			expect(lookupProcessIdentity(receipt.helper)).toBeNull();
			expect(
				runnerIdentity === null ||
					sameIdentity(lookupProcessIdentity(runnerIdentity.pid), runnerIdentity),
			).toBe(false);
		} finally {
			clearTimeout(guard);
			try {
				const receipt = browserReceiptSchema.parse(
					JSON.parse(readFileSync(join(root, 'browser-ready.json'), 'utf8')),
				);
				for (const pid of [receipt.browser, receipt.helper]) {
					const identity = lookupProcessIdentity(pid);
					if (identity !== null) owned.push(identity);
				}
			} catch {
				/* A startup failure can precede the controlled receipt. */
			}
			if (runnerIdentity !== null) owned.push(runnerIdentity);
			for (const recorded of owned)
				if (sameIdentity(lookupProcessIdentity(recorded.pid), recorded))
					process.kill(recorded.pid, 'SIGKILL');
			await completion;
			rmSync(root, { recursive: true, force: true });
		}
	},
);
