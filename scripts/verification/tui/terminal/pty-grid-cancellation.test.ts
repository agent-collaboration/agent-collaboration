/** Real OS signals cross a live owner and actual child pipe; no product UI claim. */
import { spawn } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { createInterface } from 'node:readline';
import { fileURLToPath } from 'node:url';

import { expect, test } from 'vitest';

import { controlledExecutable } from '../fixtures/controlled-child-fixtures.ts';
import { temporaryRoot } from '../fixtures/receipt-fixtures.ts';
import { lookupProcessIdentity } from '../processes/process-identity.ts';

test.each(['SIGINT', 'SIGTERM'] as const)(
	'%s stops the owned PTY protocol and flushes its transcript',
	async (signal) => {
		const directory = temporaryRoot(),
			stopped = join(directory, 'stop-seen'),
			pidPath = join(directory, 'driver-pid');
		const executable = controlledExecutable(
			directory,
			'signal-protocol-control',
			`
const fs=require('node:fs');fs.writeFileSync(${JSON.stringify(pidPath)},String(process.pid));
const bytes=s=>process.stdout.write('TUI_GRID BYTES '+Buffer.from(s).toString('hex')+'\\n');
process.stdout.write('TUI_GRID READY 120 36 STREAM\\n');
require('node:readline').createInterface({input:process.stdin}).on('line',line=>{
 if(line.startsWith('write ')){bytes('\\x1b[?2026hbefore signal\\x1b[?2026l\\x1b]7770;dispatch=1\\x1b\\\\');process.stdout.write('TUI_GRID OK\\n');}
 else if(line==='stop'){fs.writeFileSync(${JSON.stringify(stopped)},'actual stop command');bytes('\\x1b[2;1Hflushed on stop');process.stdout.write('TUI_GRID EOF\\nTUI_GRID STOPPED 0\\n',()=>process.exit(0));}
});process.stdin.on('end',()=>process.exit(0));`,
		);
		const child = spawn(
			process.execPath,
			[
				fileURLToPath(new URL('../fixtures/pty-signal-child.ts', import.meta.url)),
				executable,
				directory,
			],
			{ stdio: ['ignore', 'pipe', 'pipe'] },
		);
		let stderr = '';
		child.stderr.on('data', (bytes: Buffer) => {
			stderr += bytes.toString();
		});
		const exit = new Promise<{
			readonly code: number | null;
			readonly signal: NodeJS.Signals | null;
		}>((resolve, reject) => {
			child.once('error', reject);
			child.once('exit', (code, received) => resolve({ code, signal: received }));
		});
		const lines = createInterface({ input: child.stdout });
		try {
			await new Promise<void>((resolve, reject) => {
				lines.on('line', (line) => {
					if (line === 'SIGNAL_READY') resolve();
				});
				child.once('exit', () =>
					reject(new Error('signal fixture exited before its real marker: ' + stderr)),
				);
				child.once('error', reject);
			});
			expect(child.kill(signal)).toBe(true);
			expect(await exit).toEqual({ code: signal === 'SIGINT' ? 130 : 143, signal: null });
			expect(stderr).toContain('operator-abort: ' + signal);
			expect(readFileSync(stopped, 'utf8')).toBe('actual stop command');
			expect(readFileSync(join(directory, 'terminal.ansi'), 'utf8')).toContain('flushed on stop');
			expect(JSON.parse(readFileSync(join(directory, 'driver-commands.json'), 'utf8'))).toContain(
				'stop',
			);
			expect(existsSync(join(directory, 'driver.stdout'))).toBe(true);
			expect(lookupProcessIdentity(Number(readFileSync(pidPath, 'utf8')))).toBeNull();
		} finally {
			lines.close();
			if (child.exitCode === null && child.signalCode === null) {
				child.kill('SIGTERM');
				await exit;
			}
		}
	},
);
