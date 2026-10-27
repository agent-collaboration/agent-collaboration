import { spawn } from 'node:child_process';
import type { ChildProcess, SpawnOptions } from 'node:child_process';
import { closeSync, lstatSync, mkdtempSync, openSync, readFileSync, writeFileSync } from 'node:fs';
import { constants, tmpdir } from 'node:os';
import { join } from 'node:path';

import { z } from 'zod';

export const staticBudgetSchema = z.strictObject({
	work_seconds: z.number().positive(),
	cleanup_seconds: z.number().positive(),
});
export type StaticCaptureBudget = z.infer<typeof staticBudgetSchema>;
export interface DriverChild {
	readonly process: ChildProcess;
	readonly completion: Promise<number>;
}
export function startDriver(command: readonly string[], options: SpawnOptions): DriverChild {
	const [executable, ...argumentsList] = command;
	if (executable === undefined) throw new Error('empty driver command');
	const child = spawn(executable, argumentsList, options);
	const completion = new Promise<number>((resolve, reject) => {
		child.once('error', reject);
		child.once('close', (code, signal) => {
			resolve(code ?? -(signal === null ? 1 : constants.signals[signal]));
		});
	});
	return { process: child, completion };
}
function waitBeforeDeadline(completion: Promise<number>, seconds: number): Promise<number | null> {
	return new Promise((resolve, reject) => {
		const timer = setTimeout(() => resolve(null), seconds * 1000);
		completion.then(
			(result) => {
				clearTimeout(timer);
				resolve(result);
			},
			(error: unknown) => {
				clearTimeout(timer);
				reject(error);
			},
		);
	});
}
export function requestStopFile(marker: string): void {
	let descriptor: number;
	try {
		descriptor = openSync(marker, 'wx', 0o600);
	} catch (error) {
		if (!(error instanceof Error && 'code' in error && error.code === 'EEXIST')) throw error;
		const metadata = lstatSync(marker);
		if (
			!metadata.isFile() ||
			metadata.uid !== process.getuid?.() ||
			readFileSync(marker, 'utf8') !== 'stop\n'
		)
			throw new Error('invalid private capture stop marker', { cause: error });
		return;
	}
	try {
		writeFileSync(descriptor, 'stop\n');
	} finally {
		closeSync(descriptor);
	}
}
export async function waitStaticCapture(
	child: DriverChild,
	output: string,
	budget: StaticCaptureBudget,
	stopFile: string = join(output, 'capture-stop'),
): Promise<number> {
	const started = new Date();
	let timedOut = false;
	let stopError: string | null = null;
	let result = await waitBeforeDeadline(child.completion, budget.work_seconds);
	if (result === null) {
		timedOut = true;
		try {
			requestStopFile(stopFile);
		} catch (error) {
			stopError = String(error);
		}
		result = await waitBeforeDeadline(child.completion, budget.cleanup_seconds);
	}
	writeFileSync(
		join(output, 'driver-exit.json'),
		JSON.stringify(
			{
				exit: result,
				pid: child.process.pid,
				started_at: started.toISOString(),
				observed_at: new Date().toISOString(),
				work_seconds: budget.work_seconds,
				cleanup_seconds: budget.cleanup_seconds,
				work_deadline: new Date(started.getTime() + budget.work_seconds * 1000).toISOString(),
				whole_deadline: new Date(
					started.getTime() + (budget.work_seconds + budget.cleanup_seconds) * 1000,
				).toISOString(),
				deadline_reached: timedOut,
				driver_settled: result !== null,
				stop_error: stopError,
			},
			null,
			2,
		),
	);
	if (result === null) child.process.unref();
	if (timedOut)
		throw new Error(
			`static capture deadline; ${result === null ? 'cleanup unsettled' : 'driver exited'}; stop_error=${stopError}`,
		);
	if (result === null) throw new Error('driver exit missing');
	return result;
}

interface CapturedResult {
	readonly exit: number | null;
	readonly timedOut: boolean;
	readonly pid: number | undefined;
	readonly stdout: Buffer;
	readonly stderr: Buffer;
	readonly stdoutPath: string;
	readonly stderrPath: string;
}
async function captureCommand(
	command: readonly string[],
	repository: string,
	timeoutSeconds: number,
): Promise<CapturedResult> {
	const [executable] = command;
	if (executable === undefined) throw new Error('empty command');
	const directory = mkdtempSync(join(tmpdir(), '2026-10-quota-command-'));
	const stdoutPath = join(directory, 'stdout');
	const stderrPath = join(directory, 'stderr');
	const stdoutDescriptor = openSync(stdoutPath, 'wx', 0o600);
	const stderrDescriptor = openSync(stderrPath, 'wx', 0o600);
	try {
		// File descriptors cannot hold the parent hostage when a descendant inherits output.
		const child = startDriver(command, {
			cwd: repository,
			stdio: ['ignore', stdoutDescriptor, stderrDescriptor],
		});
		let exit = await waitBeforeDeadline(child.completion, timeoutSeconds);
		const timedOut = exit === null;
		if (timedOut) {
			child.process.kill('SIGKILL');
			exit = await waitBeforeDeadline(child.completion, 2);
		}
		if (exit === null) child.process.unref();
		return {
			exit,
			timedOut,
			pid: child.process.pid,
			stdoutPath,
			stderrPath,
			stdout: readFileSync(stdoutPath),
			stderr: readFileSync(stderrPath),
		};
	} finally {
		closeSync(stdoutDescriptor);
		closeSync(stderrDescriptor);
		// Retain raw evidence: exact-child exit cannot prove inherited writers have settled.
	}
}
export async function discoverCaptureTest(
	command: readonly string[],
	repository: string,
	timeoutSeconds: number = 10,
): Promise<string> {
	const result = await captureCommand(command, repository, timeoutSeconds);
	if (result.timedOut || result.exit !== 0)
		throw new Error(
			'capture discovery failed: ' +
				JSON.stringify({
					command,
					pid: result.pid,
					stdout_path: result.stdoutPath,
					stderr_path: result.stderrPath,
					exit: result.exit,
					timeout: result.timedOut,
					stdout: result.stdout.toString(),
					stderr: result.stderr.toString(),
					stdout_base64: result.stdout.toString('base64'),
					stderr_base64: result.stderr.toString('base64'),
				}),
		);
	return result.stdout.toString();
}
export async function runCapturedCommand(options: {
	readonly command: readonly string[];
	readonly repository: string;
	readonly timeoutSeconds: number;
}): Promise<{ readonly exit: number; readonly stdout: string; readonly stderr: string }> {
	const result = await captureCommand(options.command, options.repository, options.timeoutSeconds);
	if (result.timedOut || result.exit === null)
		throw new Error(
			'command reap deadline; ' +
				JSON.stringify({
					command: options.command,
					pid: result.pid,
					stdout_path: result.stdoutPath,
					stderr_path: result.stderrPath,
					exit: result.exit,
					timeout: result.timedOut,
					stdout_base64: result.stdout.toString('base64'),
					stderr_base64: result.stderr.toString('base64'),
				}),
		);
	return { exit: result.exit, stdout: result.stdout.toString(), stderr: result.stderr.toString() };
}
