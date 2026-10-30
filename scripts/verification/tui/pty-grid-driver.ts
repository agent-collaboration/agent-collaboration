/** Grid observer over the existing Rust TerminalDriver. One owner of each PTY. */
import { spawn } from 'node:child_process';
import type { ChildProcessWithoutNullStreams } from 'node:child_process';
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';

import type { GhosttyCore } from '@wterm/ghostty';

import { createTerminalCore, gridLines, snapshotGrid } from './terminal-grid.ts';
import type { TerminalGrid, TerminalResize } from './terminal-grid.ts';

export type KeyProtocol = 'legacy' | 'kitty';
// One native read's bound, and the default checkpoint bound for every existing consumer.
const READ_INTERVAL_MILLIS = 8_000;
// drive_terminal's reply when a bounded read saw no bytes (std RecvTimeoutError::Timeout).
const QUIET_READ = 'READ_ERROR timed out waiting on channel';
interface WaitingLine {
	readonly resolve: (line: string) => void;
	readonly reject: (error: Error) => void;
	readonly timer: ReturnType<typeof setTimeout>;
}
export interface PtyGridTarget {
	readonly repository: string;
	readonly driver: string;
	readonly surface:
		| 'sessions'
		| 'sessions-filters'
		| 'quota'
		| 'quota-filters'
		| 'normal'
		| 'hard'
		| 'public-sessions'
		| 'codex-acp';
	readonly keys: KeyProtocol;
	readonly evidence: string;
	readonly componentBinary?: string;
	readonly publicEnvironment?: NodeJS.ProcessEnv;
}
export class PtyGridDriver {
	private readonly child: ChildProcessWithoutNullStreams;
	private readonly core: GhosttyCore;
	private readonly target: PtyGridTarget;
	private readonly lines: string[] = [];
	private waiter: WaitingLine | null = null;
	private readonly raw: Buffer[] = [];
	private readonly resizes: TerminalResize[] = [];
	private readonly commands: string[] = [];
	private ended = false;
	private readonly completion: Promise<number | null>;
	private released: Promise<number | null> | null = null;
	private constructor(target: PtyGridTarget, core: GhosttyCore) {
		this.target = target;
		this.core = core;
		this.child = spawn(
			target.driver,
			[
				target.surface === 'public-sessions'
					? 'compiled_public_sessions_grid_observation'
					: target.surface === 'codex-acp'
						? 'compiled_codex_acp_grid_observation'
						: 'compiled_grid_terminal_observation',
				'--exact',
				'--ignored',
				'--nocapture',
				'--test-threads=1',
			],
			{
				cwd: target.repository,
				env: {
					...process.env,
					...(target.surface === 'public-sessions' || target.surface === 'codex-acp'
						? target.publicEnvironment
						: {}),
					TUI_GRID_SURFACE: target.surface,
					TUI_GRID_KEYS: target.keys,
					...(target.componentBinary === undefined
						? {}
						: {
								TUI_GRID_COMPONENT_BINARY: target.componentBinary,
								TUI_GRID_COMPONENT_RECEIPT: join(target.evidence, 'fixture-finished'),
							}),
				},
			},
		);
		let pending = '';
		let stdout = '';
		let stderr = '';
		this.child.stdout.on('data', (chunk: Buffer) => {
			pending += chunk.toString('utf8');
			stdout += chunk.toString('utf8');
			while (pending.includes('\n')) {
				const end = pending.indexOf('\n');
				const line = pending.slice(0, end);
				pending = pending.slice(end + 1);
				const marker = line.indexOf('TUI_GRID ');
				if (marker < 0) continue;
				const response = line.slice(marker + 9);
				if (this.waiter !== null) {
					clearTimeout(this.waiter.timer);
					this.waiter.resolve(response);
					this.waiter = null;
				} else this.lines.push(response);
			}
		});
		this.child.stderr.on('data', (chunk: Buffer) => {
			stderr += chunk.toString('utf8');
		});
		this.completion = new Promise((resolve) => {
			this.child.once('error', (error) => {
				if (this.waiter !== null) {
					clearTimeout(this.waiter.timer);
					this.waiter.reject(error);
					this.waiter = null;
				}
			});
			this.child.once('close', (code) => {
				this.ended = true;
				writeFileSync(join(target.evidence, 'driver.stdout'), stdout);
				writeFileSync(join(target.evidence, 'driver.stderr'), stderr);
				if (this.waiter !== null) {
					clearTimeout(this.waiter.timer);
					this.waiter.reject(new Error(`PTY driver exited ${code}: ${stderr}`));
					this.waiter = null;
				}
				resolve(code);
			});
		});
	}
	static async start(target: PtyGridTarget): Promise<PtyGridDriver> {
		const core = await createTerminalCore(120, 36);
		const driver = new PtyGridDriver(target, core);
		try {
			const ready = await driver.nextLine();
			if (ready !== 'READY 120 36') throw new Error(`PTY readiness failed: ${ready}`);
			return driver;
		} catch (error) {
			await driver.stop();
			throw error;
		}
	}
	private nextLine(): Promise<string> {
		const queued = this.lines.shift();
		if (queued !== undefined) return Promise.resolve(queued);
		if (this.ended) return Promise.reject(new Error('PTY driver already ended'));
		return new Promise((resolve, reject) => {
			const timer = setTimeout(() => {
				this.waiter = null;
				reject(new Error('PTY command deadline'));
			}, 30_000);
			this.waiter = { resolve, reject, timer };
		});
	}
	private async command(command: string): Promise<string> {
		this.commands.push(command);
		this.child.stdin.write(`${command}\n`);
		return this.nextLine();
	}
	async send(bytes: Uint8Array): Promise<void> {
		if (bytes.length === 0) return;
		const response = await this.command(`write ${Buffer.from(bytes).toString('hex')}`);
		if (response !== 'OK') throw new Error(response);
	}
	async resize(columns: number, rows: number): Promise<void> {
		const response = await this.command(`resize ${columns} ${rows}`);
		if (response !== 'OK') throw new Error(response);
		this.core.resize(columns, rows);
		this.resizes.push({ byteOffset: this.bytes().length, columns, rows });
	}
	async receive(timeoutMillis = 8_000): Promise<'bytes' | 'eof'> {
		const response = await this.command(`read ${timeoutMillis}`);
		if (response === 'EOF') return 'eof';
		if (!response.startsWith('BYTES ')) throw new Error(response);
		const bytes = Buffer.from(response.slice(6), 'hex');
		this.raw.push(bytes);
		this.core.writeRaw(bytes);
		const replies: string[] = [];
		for (let reply = this.core.getResponse(); reply !== null; reply = this.core.getResponse()) {
			const isKitty = reply.startsWith('\x1b[?') && reply.endsWith('u');
			if (this.target.keys !== 'legacy' || !isKitty) replies.push(reply);
		}
		if (replies.length > 0) {
			const result = await this.command(`reply ${Buffer.from(replies.join('')).toString('hex')}`);
			if (result !== 'OK') throw new Error(result);
		}
		return 'bytes';
	}
	grid(): TerminalGrid {
		return snapshotGrid(this.core);
	}
	bytes(): Buffer {
		return Buffer.concat(this.raw);
	}
	resizeHistory(): readonly TerminalResize[] {
		return [...this.resizes];
	}
	frameGeneration(): number {
		return this.core.synchronizedOutputGeneration();
	}
	/**
	 * Waits for a matching frame. A checkpoint longer than the 8 s default (a real model reply)
	 * spans successive reads, each still bounded by 8 s; a read that only times out quietly
	 * before that checkpoint's deadline waits again instead of failing it.
	 */
	async waitForGrid(
		name: string,
		matches: (grid: TerminalGrid) => boolean,
		after = -1,
		checkpointMillis = READ_INTERVAL_MILLIS,
	): Promise<TerminalGrid> {
		const deadline = Date.now() + checkpointMillis;
		const quietRead = async (): Promise<'bytes' | 'eof' | 'quiet'> => {
			try {
				return await this.receive(
					Math.max(1, Math.min(deadline - Date.now(), READ_INTERVAL_MILLIS)),
				);
			} catch (error) {
				if (
					checkpointMillis > READ_INTERVAL_MILLIS &&
					Date.now() < deadline &&
					error instanceof Error &&
					error.message === QUIET_READ
				)
					return 'quiet';
				throw error;
			}
		};
		const nextFrame = async (): Promise<TerminalGrid> => {
			if (!this.core.synchronizedOutput() && this.frameGeneration() > after && matches(this.grid()))
				return this.grid();
			if (Date.now() >= deadline || (await quietRead()) === 'eof') {
				throw new Error(
					`grid checkpoint unreachable: ${name}\n${gridLines(this.grid()).join('\n')}`,
				);
			}
			return nextFrame();
		};
		return nextFrame();
	}
	async running(): Promise<boolean> {
		return (await this.command('running')) === 'RUNNING';
	}
	async finish(): Promise<number | null> {
		const response = await this.command('finish');
		if (!response.startsWith('FINISHED ')) throw new Error(response);
		return this.release();
	}
	async stop(): Promise<number | null> {
		if (!this.ended) {
			try {
				await this.command('stop');
			} catch {
				if (!this.ended) this.child.stdin.end();
			}
		}
		return this.release();
	}
	private async release(): Promise<number | null> {
		this.released ??= this.settle();
		return this.released;
	}
	private async settle(): Promise<number | null> {
		const code = await this.completion;
		writeFileSync(join(this.target.evidence, 'terminal.ansi'), this.bytes());
		writeFileSync(
			join(this.target.evidence, 'driver-commands.json'),
			JSON.stringify(this.commands, null, 2),
		);
		writeFileSync(
			join(this.target.evidence, 'terminal-grid.json'),
			JSON.stringify(this.grid(), null, 2),
		);
		this.core.dispose();
		return code;
	}
}
