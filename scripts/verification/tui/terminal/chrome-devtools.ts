/** The owned Chrome process uses DevTools pipes; no browser automation dependency. */
import { spawn } from 'node:child_process';
import type { ChildProcess } from 'node:child_process';
import { mkdtempSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { PassThrough } from 'node:stream';

import { z } from 'zod';

const responseSchema = z.object({
	['id']: z.number().optional(),
	result: z.unknown().optional(),
	error: z.object({ message: z.string() }).optional(),
	method: z.string().optional(),
	sessionId: z.string().optional(),
});
interface PendingCommand {
	readonly resolve: (value: unknown) => void;
	readonly reject: (reason: Error) => void;
	readonly timer: ReturnType<typeof setTimeout>;
}
export class ChromeDevtools {
	readonly child: ChildProcess;
	readonly profile: string;
	private readonly input: PassThrough;
	private readonly pending = new Map<number, PendingCommand>();
	private readonly events = new Map<string, PendingCommand>();
	private failure: Error | null = null;
	private nextId = 0;
	private readonly stopped: Promise<number | null>;
	private readonly pipesClosed: Promise<void>;
	constructor(binary: string, evidence: string) {
		this.profile = mkdtempSync(join(evidence, 'chrome-profile-'));
		this.input = new PassThrough();
		const output = new PassThrough();
		this.child = spawn(
			binary,
			[
				'--headless=new',
				'--disable-gpu',
				'--no-first-run',
				'--disable-background-networking',
				'--remote-debugging-pipe',
				`--user-data-dir=${this.profile}`,
				'about:blank',
			],
			// A private process group keeps capture helpers inside this owner's shutdown boundary.
			{ detached: true, stdio: ['ignore', 'ignore', 'pipe', 'pipe', 'pipe'] },
		);
		this.pipesClosed = new Promise((resolve) => this.child.once('close', () => resolve()));
		const pipeIn = this.child.stdio[3];
		const pipeOut = this.child.stdio[4];
		if (
			pipeIn === null ||
			pipeIn === undefined ||
			!('write' in pipeIn) ||
			pipeOut === null ||
			pipeOut === undefined ||
			!('pipe' in pipeOut)
		) {
			this.child.kill();
			throw new Error('Chrome DevTools pipes unavailable');
		}
		this.input.pipe(pipeIn);
		pipeOut.pipe(output);
		const fail = (error: Error): void => {
			this.failure = error;
			for (const entry of [...this.pending.values(), ...this.events.values()]) {
				clearTimeout(entry.timer);
				entry.reject(error);
			}
			this.pending.clear();
			this.events.clear();
		};
		pipeIn.on('error', fail);
		pipeOut.on('error', fail);
		let buffer = '';
		output.on('data', (data: Buffer) => {
			buffer += data.toString('utf8');
			while (buffer.includes('\0')) {
				const end = buffer.indexOf('\0');
				const response = responseSchema.parse(JSON.parse(buffer.slice(0, end)));
				buffer = buffer.slice(end + 1);
				if (response.id === undefined) {
					const key = `${response.sessionId}:${response.method}`;
					const event = this.events.get(key);
					if (event !== undefined) {
						clearTimeout(event.timer);
						this.events.delete(key);
						event.resolve(response);
					}
					continue;
				}
				const pending = this.pending.get(response.id);
				if (pending === undefined) continue;
				this.pending.delete(response.id);
				clearTimeout(pending.timer);
				if (response.error !== undefined) pending.reject(new Error(response.error.message));
				else pending.resolve(response.result);
			}
		});
		let stderr = '';
		this.child.stderr?.on('data', (data: Buffer) => {
			stderr += data.toString('utf8');
		});
		this.stopped = new Promise((resolve) => {
			this.child.once('exit', (code) => {
				writeFileSync(join(evidence, 'chrome.stderr'), stderr);
				fail(new Error(`Chrome exited ${code}; ${stderr.trim()}`));
				resolve(code);
			});
			this.child.once('error', (error) => {
				writeFileSync(join(evidence, 'chrome.stderr'), `${stderr}\n${error.message}`);
				fail(error);
				resolve(null);
			});
		});
	}
	send(
		method: string,
		params: Readonly<Record<string, unknown>> = {},
		sessionId?: string,
	): Promise<unknown> {
		if (this.failure !== null) return Promise.reject(this.failure);
		const commandId = ++this.nextId;
		return new Promise((resolve, reject) => {
			const timer = setTimeout(() => {
				this.pending.delete(commandId);
				reject(new Error(`DevTools deadline: ${method}`));
			}, 15_000);
			this.pending.set(commandId, { resolve, reject, timer });
			this.input.write(
				`${JSON.stringify({ ['id']: commandId, method, params, ...(sessionId === undefined ? {} : { sessionId }) })}\0`,
			);
		});
	}
	waitForEvent(method: string, sessionId: string): Promise<unknown> {
		if (this.failure !== null) return Promise.reject(this.failure);
		const key = `${sessionId}:${method}`;
		return new Promise((resolve, reject) => {
			const timer = setTimeout(() => {
				this.events.delete(key);
				reject(new Error(`DevTools event deadline: ${method}`));
			}, 15_000);
			this.events.set(key, { resolve, reject, timer });
		});
	}
	async close(): Promise<number | null> {
		try {
			await this.send('Browser.close');
		} catch {
			if (this.child.exitCode === null) this.child.kill('SIGTERM');
		}
		const code = await this.stopped;
		// Browser.exit precedes close when a helper still owns inherited descriptors.
		// The group belongs only to the detached browser we spawned, including its helpers.
		if (this.child.pid !== undefined) {
			try {
				process.kill(-this.child.pid, 'SIGKILL');
			} catch (error) {
				if (!(error instanceof Error && 'code' in error && error.code === 'ESRCH')) throw error;
			}
		}
		await this.pipesClosed;
		return code;
	}
}
