/** Grid observer over the existing Rust TerminalDriver. One owner of each PTY. */
import { spawn } from 'node:child_process';
import type { ChildProcessWithoutNullStreams } from 'node:child_process';
import { lstatSync, realpathSync, writeFileSync } from 'node:fs';
import { basename, dirname, isAbsolute, join } from 'node:path';

import type { GhosttyCore } from '@wterm/ghostty';

import type { ArtifactBinding } from '../evidence/evidence-schema.ts';
import { PENDING_SCENARIOS, isComponentScenario } from '../fixtures/component-fixture-contract.ts';
import type { ComponentScenario } from '../fixtures/component-fixture-contract.ts';
import {
	SESSIONS_PREFERENCES_DIRECTORY,
	sessionsPreferencesEnvironment,
} from '../sessions/sessions-preferences-directory.ts';
import {
	SESSIONS_RECEIPT_DIRECTORY,
	sessionsReceiptEnvironment,
} from '../sessions/sessions-receipt-directory.ts';
import type { TerminalInputModes } from './ghostty-input-encoding.ts';
import { readGhosttyInputModes } from './ghostty-input-encoding.ts';
import { DispatchFrameIndex, dispatchFrameGrid } from './input-dispatch-frames.ts';
import type { DispatchFrameObservation } from './input-dispatch-frames.ts';
import { createTerminalCore, gridLines, snapshotGrid } from './terminal-grid.ts';
import type { TerminalGrid, TerminalResize } from './terminal-grid.ts';

export type KeyProtocol = 'legacy' | 'kitty';
interface WaitingLine {
	readonly resolve: (line: string) => void;
	readonly reject: (error: Error) => void;
	readonly timer: ReturnType<typeof setTimeout>;
}
export interface PtyGridTarget {
	readonly node?: ArtifactBinding;
	readonly product?: ArtifactBinding;
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
		| 'showcase'
		| 'quota-effects';
	readonly keys: KeyProtocol;
	readonly evidence: string;
	readonly componentBinary?: string;
	readonly componentDispatchReceipt?: string;
	readonly componentScenario?: ComponentScenario;
	readonly componentReleaseFile?: string;
	readonly sessionsReceiptDirectory?: string;
	readonly sessionsPreferencesDirectory?: string;
	readonly publicEnvironment?: NodeJS.ProcessEnv;
	readonly showcaseEnvironment?: NodeJS.ProcessEnv;
	readonly quotaEffectEnvironment?: NodeJS.ProcessEnv;
	readonly handshakeOnly?: boolean;
	readonly storyboardVariant?:
		| 'wide-glyph-left-border'
		| 'source-qualified-outcome'
		| 'configured-source-new'
		| 'stopped-native-claude'
		| 'local-codex-new'
		| 'new-machine-uncommitted'
		| 'source-options-add'
		| 'source-options-remove'
		| 'source-options-replace'
		| 'fork-ssh-profile'
		| 'claude-new-success'
		| 'large-list'
		| 'r19-grouping';
}
export function ptyGridDriverEntry(surface: PtyGridTarget['surface']): string {
	switch (surface) {
		case 'quota':
		case 'quota-filters':
		case 'quota-effects':
		case 'normal':
		case 'hard':
			return 'quota_storyboard::compiled_grid_terminal_observation';
		case 'public-sessions':
			return 'compiled_public_sessions_grid_observation';
		case 'sessions':
		case 'sessions-filters':
		case 'showcase':
			return 'compiled_grid_terminal_observation';
	}
	const exhaustive: never = surface;
	throw new Error('unknown driver surface: ' + String(exhaustive));
}
export function ptyGridDriverArguments(surface: PtyGridTarget['surface']): readonly string[] {
	return [ptyGridDriverEntry(surface), '--exact', '--ignored', '--nocapture', '--test-threads=1'];
}
function componentEnvironment(target: PtyGridTarget): Readonly<Record<string, string>> {
	if (
		target.componentDispatchReceipt === undefined &&
		target.componentScenario === undefined &&
		target.componentReleaseFile === undefined
	)
		return {};
	if (target.componentBinary === undefined || !['normal', 'hard'].includes(target.surface))
		throw new Error('component receipt/scenario requires a component fixture');
	const scenario = target.componentScenario;
	if (
		scenario !== undefined &&
		(!isComponentScenario(scenario) ||
			(target.surface === 'normal'
				? scenario === 'visual-hard-cases'
				: scenario !== 'visual-hard-cases'))
	)
		throw new Error('component scenario does not match its surface');
	const release = target.componentReleaseFile;
	const pending = scenario !== undefined && PENDING_SCENARIOS.has(scenario);
	if (release !== undefined && !pending)
		throw new Error('component release requires a pending scenario');
	if (pending && release === undefined)
		throw new Error('pending scenario requires a fresh owned release path');
	const paths = [target.componentDispatchReceipt, release].filter((path) => path !== undefined);
	if (new Set(paths).size !== paths.length) throw new Error('component output paths collide');
	for (const path of paths) validateFreshComponentPath(target.evidence, path);
	return {
		...(target.componentDispatchReceipt === undefined
			? {}
			: { TUI_GRID_COMPONENT_DISPATCH_RECEIPT: target.componentDispatchReceipt }),
		...(scenario === undefined ? {} : { TUI_GRID_COMPONENT_SCENARIO: scenario }),
		...(release === undefined ? {} : { TUI_GRID_COMPONENT_RELEASE: release }),
	};
}
function validateFreshComponentPath(evidence: string, receipt: string): void {
	const metadata = lstatSync(evidence);
	if (
		!metadata.isDirectory() ||
		metadata.isSymbolicLink() ||
		metadata.uid !== process.getuid?.() ||
		!isAbsolute(receipt) ||
		realpathSync(dirname(receipt)) !== realpathSync(evidence) ||
		receipt !== join(realpathSync(evidence), basename(receipt))
	)
		throw new Error('component receipt requires a fresh path directly inside owned output');
	try {
		lstatSync(receipt);
	} catch (error) {
		if (!(error instanceof Error && 'code' in error && error.code === 'ENOENT')) throw error;
		return;
	}
	throw new Error('component receipt path already exists');
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
	private commandTail: Promise<void> = Promise.resolve();
	private outputRevision = 0;
	private receivedRevision = 0;
	private eof = false;
	private failure: Error | null = null;
	private readonly outputWaiters = new Set<() => void>();
	private readonly dispatchFrames = new DispatchFrameIndex();
	private constructor(target: PtyGridTarget, core: GhosttyCore) {
		this.target = target;
		this.core = core;
		if (
			target.sessionsReceiptDirectory !== undefined &&
			!['sessions', 'sessions-filters'].includes(target.surface)
		) {
			throw new Error('sessions receipt directory requires a sessions fixture surface');
		}
		const componentOverrides = componentEnvironment(target);
		if (target.handshakeOnly === true && target.surface !== 'showcase')
			throw new Error('wrapper handshake requires showcase');
		const receiptEnvironment = sessionsReceiptEnvironment(
			target.sessionsReceiptDirectory,
			target.evidence,
		);
		const preferencesEnvironment = sessionsPreferencesEnvironment(
			target.sessionsPreferencesDirectory,
			target.repository,
			target.surface,
		);
		this.child = spawn(target.driver, ptyGridDriverArguments(target.surface), {
			cwd: target.repository,
			env: {
				...process.env,
				TUI_GRID_COMPONENT_DISPATCH_RECEIPT: undefined,
				TUI_GRID_COMPONENT_SCENARIO: undefined,
				TUI_GRID_COMPONENT_RELEASE: undefined,
				QUOTA_COMPONENT_FIXTURE_DISPATCH_RECEIPT_FILE: undefined,
				QUOTA_COMPONENT_FIXTURE_RELEASE_FILE: undefined,
				...componentOverrides,
				TUI_GRID_PRODUCT_BINARY: target.product?.path,
				TUI_GRID_PRODUCT_SHA256: target.product?.sha256,
				...(target.node === undefined
					? {}
					: {
							QUOTA_STORYBOARD_NODE: target.node.path,
							QUOTA_STORYBOARD_NODE_SHA256: target.node.sha256,
						}),
				[SESSIONS_RECEIPT_DIRECTORY]: undefined,
				...receiptEnvironment,
				[SESSIONS_PREFERENCES_DIRECTORY]: undefined,
				...preferencesEnvironment,
				...(target.surface === 'public-sessions' ? target.publicEnvironment : {}),
				...(target.surface === 'showcase' ? target.showcaseEnvironment : {}),
				...(target.surface === 'quota-effects' ? target.quotaEffectEnvironment : {}),
				TUI_GRID_CHECK_ONLY: undefined,
				TUI_GRID_HANDSHAKE_ONLY: target.handshakeOnly === true ? '1' : undefined,
				TUI_GRID_SURFACE: target.surface,
				TUI_GRID_KEYS: target.keys,
				...(target.storyboardVariant === undefined
					? {}
					: { TUI_GRID_STORYBOARD_VARIANT: target.storyboardVariant }),
				...(target.componentBinary === undefined
					? {}
					: {
							TUI_GRID_COMPONENT_BINARY: target.componentBinary,
							TUI_GRID_COMPONENT_RECEIPT: join(target.evidence, 'fixture-finished'),
						}),
			},
		});
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
				if (response.startsWith('BYTES ')) {
					try {
						this.acceptBytes(response.slice(6));
					} catch (error) {
						this.failStream(error);
					}
					continue;
				}
				if (response === 'EOF') {
					this.eof = true;
					this.publishOutput();
					continue;
				}
				if (response.startsWith('RESIZED ')) {
					const [columns, rows] = response.slice(8).split(' ').map(Number);
					if (
						columns === undefined ||
						rows === undefined ||
						!Number.isSafeInteger(columns) ||
						!Number.isSafeInteger(rows) ||
						columns <= 0 ||
						rows <= 0 ||
						columns > 65535 ||
						rows > 65535
					)
						this.failStream(new Error('invalid resize stream record'));
					else {
						this.core.resize(columns, rows);
						this.resizes.push({ byteOffset: this.bytes().length, columns, rows });
					}
					continue;
				}
				if (response.startsWith('READ_ERROR ')) {
					this.failStream(new Error(response));
					continue;
				}
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
		this.child.stdin.on('error', (error) => this.failStream(error));
		this.completion = new Promise((resolve) => {
			this.child.once('error', (error) => {
				this.failStream(error);
				if (this.waiter !== null) {
					clearTimeout(this.waiter.timer);
					this.waiter.reject(error);
					this.waiter = null;
				}
			});
			this.child.once('close', (code) => {
				this.ended = true;
				this.publishOutput();
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
		let driver: PtyGridDriver;
		try {
			driver = new PtyGridDriver(target, core);
		} catch (error) {
			core.dispose();
			throw error;
		}
		try {
			const ready = await driver.nextLine();
			if (ready !== 'READY 120 36 STREAM')
				throw new Error(`streaming PTY driver required: ${ready}`);
			return driver;
		} catch (error) {
			await driver.stop();
			throw error;
		}
	}
	processId(): number {
		const pid = this.child.pid;
		if (pid === undefined) throw new Error('owned Rust driver PID absent');
		return pid;
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
		const result = this.commandTail.then(async () => {
			if (this.failure !== null && command !== 'stop') throw this.failure;
			if (this.ended || this.child.stdin.destroyed)
				throw new Error('PTY input ended before command');
			this.commands.push(command);
			const response = this.nextLine();
			this.child.stdin.write(`${command}\n`);
			return response;
		});
		this.commandTail = result.then(
			() => undefined,
			() => undefined,
		);
		return result;
	}
	async send(bytes: Uint8Array): Promise<void> {
		if (bytes.length === 0) return;
		const response = await this.command(`write ${Buffer.from(bytes).toString('hex')}`);
		if (response !== 'OK') throw new Error(response);
	}
	async resize(columns: number, rows: number): Promise<void> {
		const response = await this.command(`resize ${columns} ${rows}`);
		if (response !== 'OK') throw new Error(response);
	}
	async receive(timeoutMillis = 8_000): Promise<'bytes' | 'eof'> {
		if (this.outputRevision === this.receivedRevision && !this.eof)
			await this.waitForOutput(timeoutMillis);
		if (this.failure !== null) throw this.failure;
		this.receivedRevision = this.outputRevision;
		return this.eof ? 'eof' : 'bytes';
	}
	private acceptBytes(hex: string): void {
		if (!/^(?:[0-9a-f]{2})+$/iu.test(hex)) throw new Error('invalid PTY byte record');
		if (this.eof) throw new Error('PTY bytes after EOF');
		const bytes = Buffer.from(hex, 'hex');
		this.raw.push(bytes);
		this.dispatchFrames.append(bytes);
		this.core.writeRaw(bytes);
		const replies: string[] = [];
		for (let reply = this.core.getResponse(); reply !== null; reply = this.core.getResponse()) {
			const isKitty = reply.startsWith('\x1b[?') && reply.endsWith('u');
			if (this.target.keys !== 'legacy' || !isKitty) replies.push(reply);
		}
		if (replies.length > 0) {
			void this.command(`reply ${Buffer.from(replies.join('')).toString('hex')}`).then(
				(result) => {
					if (result !== 'OK') this.failStream(new Error(result));
				},
				(error: unknown) => this.failStream(error),
			);
		}
		this.publishOutput();
	}
	private failStream(error: unknown): void {
		this.failure ??= error instanceof Error ? error : new Error(String(error));
		if (this.waiter !== null) {
			clearTimeout(this.waiter.timer);
			this.waiter.reject(this.failure);
			this.waiter = null;
		}
		this.publishOutput();
	}
	private publishOutput(): void {
		this.outputRevision += 1;
		for (const notify of this.outputWaiters) notify();
	}
	private waitForOutput(timeoutMillis: number): Promise<void> {
		if (this.failure !== null) return Promise.reject(this.failure);
		if (this.ended) return Promise.reject(new Error('PTY driver already ended'));
		return new Promise((resolve, reject) => {
			const notify = (): void => {
				clearTimeout(timer);
				this.outputWaiters.delete(notify);
				resolve();
			};
			const timer = setTimeout(() => {
				this.outputWaiters.delete(notify);
				reject(new Error('PTY output deadline'));
			}, timeoutMillis);
			this.outputWaiters.add(notify);
		});
	}
	async waitForDispatchFrame(sequence: number): Promise<DispatchFrameObservation> {
		if (this.dispatchFrames.isUnframedHandoff(sequence))
			throw new Error('loop-ending handoff has no promised continuing frame');
		const deadline = Date.now() + 8_000;
		const next = async (): Promise<DispatchFrameObservation> => {
			if (this.failure !== null) throw this.failure;
			const binding = this.dispatchFrames.get(sequence);
			if (binding !== null) {
				writeFileSync(
					join(this.target.evidence, `dispatch-${sequence}-frame.json`),
					JSON.stringify(binding, null, 2),
				);
				const bytes = this.bytes();
				const resizes = this.resizeHistory();
				return {
					binding,
					grid: await dispatchFrameGrid(bytes, resizes, binding),
					bytes: bytes.subarray(0, binding.frameEndByteOffset),
					resizes: resizes.filter((resize) => resize.byteOffset <= binding.frameEndByteOffset),
				};
			}
			const remaining = deadline - Date.now();
			if (remaining <= 0 || this.eof)
				throw new Error(`dispatch frame marker absent for sequence ${sequence}`);
			await this.waitForOutput(remaining);
			return next();
		};
		return next();
	}
	acknowledgeLoopEndingHandoff(sequence: number): void {
		this.dispatchFrames.acknowledgeLoopEndingHandoff(sequence);
	}
	grid(): TerminalGrid {
		return snapshotGrid(this.core);
	}
	/** Actual modes parsed from received PTY output; no assumed key-protocol flags. */
	inputModes(): TerminalInputModes {
		return readGhosttyInputModes(this.core);
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
	async waitForGrid(
		name: string,
		matches: (grid: TerminalGrid) => boolean,
		after = -1,
	): Promise<TerminalGrid> {
		const deadline = Date.now() + 8_000;
		const nextFrame = async (): Promise<TerminalGrid> => {
			const matched = (): boolean =>
				!this.core.synchronizedOutput() && this.frameGeneration() > after && matches(this.grid());
			if (matched()) return this.grid();
			if (
				Date.now() >= deadline ||
				(await this.receive(Math.max(1, deadline - Date.now()))) === 'eof'
			) {
				// A final complete frame and EOF can share one stdout delivery.
				if (matched()) return this.grid();
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
		// Some bridges publish their terminal result with EOF before the caller asks.
		// That retained causal result needs no write to a pipe that may already be closed.
		if (this.eof && this.lines[0]?.startsWith('FINISHED ')) {
			this.lines.shift();
			return this.release();
		}
		const response = await this.command('finish');
		if (!response.startsWith('FINISHED ')) throw new Error(response);
		return this.release();
	}
	/** EOF and the Rust driver's child-exit check prove exit; time only fails it. */
	async drainUntilExit(timeoutMillis = 8_000): Promise<number | null> {
		if (!Number.isSafeInteger(timeoutMillis) || timeoutMillis <= 0 || timeoutMillis > 30_000)
			throw new Error('apparatus exit deadline must be within 1..30000 milliseconds');
		const deadline = Date.now() + timeoutMillis;
		const drain = async (): Promise<number | null> => {
			const remaining = deadline - Date.now();
			if (remaining <= 0) throw new Error('apparatus exit deadline: no child EOF');
			try {
				if ((await this.receive(remaining)) === 'eof') return this.finish();
			} catch (error) {
				if (error instanceof Error && error.message === 'PTY output deadline')
					throw new Error('apparatus exit deadline: no child EOF', { cause: error });
				throw new Error('apparatus exit drain failed before confirmed child exit', {
					cause: error,
				});
			}
			return drain();
		};
		return drain();
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
