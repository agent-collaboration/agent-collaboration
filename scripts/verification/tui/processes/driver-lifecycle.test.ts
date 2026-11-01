import { ChildProcess } from 'node:child_process';
import { once } from 'node:events';
import { closeSync, openSync, readFileSync, rmSync, writeFileSync, existsSync } from 'node:fs';
import { createServer } from 'node:net';
import type { Socket } from 'node:net';
import { join } from 'node:path';
import { createInterface } from 'node:readline';
import type { Interface } from 'node:readline';

import { afterEach, expect, test, vi } from 'vitest';
import { z } from 'zod';

import { requireOneTest } from '../capture/capture-selection.ts';
import { readJson } from '../evidence/source-binding.ts';
import { controlledExecutable } from '../fixtures/controlled-child-fixtures.ts';
import { temporaryRoot } from '../fixtures/receipt-fixtures.ts';
import {
	discoverCaptureTest,
	runCapturedCommand,
	startDriver,
	waitStaticCapture,
} from './driver-lifecycle.ts';
import type { DriverChild } from './driver-lifecycle.ts';
const roots: string[] = [];
const children: DriverChild[] = [];
function rootDirectory(): string {
	const root = temporaryRoot();
	roots.push(root);
	return root;
}
afterEach(async () => {
	vi.useRealTimers();
	await Promise.all(
		children.splice(0).map(async (child) => {
			if (child.process.exitCode === null && child.process.signalCode === null)
				child.process.kill('SIGKILL');
			await child.completion;
		}),
	);
	for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
});
function protocolDriver(root: string, source: string): DriverChild {
	const executable = controlledExecutable(root, 'owned-driver', source);
	const stdout = openSync(join(root, 'driver.stdout'), 'w');
	const stderr = openSync(join(root, 'driver.stderr'), 'w');
	try {
		const child = startDriver([executable, root], { stdio: ['ignore', stdout, stderr, 'ipc'] });
		children.push(child);
		return child;
	} finally {
		closeSync(stdout);
		closeSync(stderr);
	}
}
test('discovery must identify only the exact declared libtest entry', async () => {
	const root = rootDirectory();
	const executable = controlledExecutable(
		root,
		'discovery',
		"process.stdout.write('declared: test\\n');",
	);
	const discovery = await discoverCaptureTest([executable, 'declared', '--exact', '--list'], root);
	requireOneTest(discovery, 'declared');
	expect(() => requireOneTest(discovery + 'other: test\n', 'declared')).toThrow('broadened');
	expect(() => requireOneTest(discovery, 'wrong')).toThrow('missing');
});
test('capture_discovery_bounds_and_reaps_its_exact_child', async () => {
	const root = rootDirectory();
	const control = await oracleReleaseControl(root);
	const executable = controlledExecutable(
		root,
		'held-discovery',
		"const fs=require('node:fs');process.stdout.write('partial list\\n');process.stderr.write('partial error\\n');fs.writeFileSync(process.argv[2]+'/discovery-ready','ready\\n');" +
			controlledOracleSocketSource(root, control.port, 0),
	);
	// The child must emit real output before this unit advances the deadline.
	// Its process, raw descriptors and exact-child SIGKILL remain real.
	vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
	const discovery = discoverCaptureTest([executable, root], root, 0.2).then(
		() => '',
		(error: unknown) => String(error),
	);
	let failure: string;
	try {
		await control.ready;
		expect(readFileSync(join(root, 'discovery-ready'), 'utf8')).toBe('ready\n');
	} finally {
		try {
			await vi.advanceTimersByTimeAsync(200);
			failure = await discovery;
		} finally {
			await control.close();
			vi.useRealTimers();
		}
	}
	expect(failure).toContain('"timeout":true');
	expect(failure).toContain('"exit":-9');
	expect(failure).toContain('partial list');
	expect(failure).toContain('partial error');
	expect(failure).toContain('stdout_base64');
});
test('capture_discovery_preserves_success_and_nonzero_output', async () => {
	const root = rootDirectory();
	const good = controlledExecutable(root, 'good-list', `process.stdout.write('exact: test\\n');`);
	expect(await discoverCaptureTest([good], root)).toBe('exact: test\n');
	const bad = controlledExecutable(
		root,
		'bad-list',
		`process.stdout.write('bad list\\n');process.stderr.write('error\\n');process.exitCode=7;`,
	);
	await expect(discoverCaptureTest([bad], root)).rejects.toThrow('"exit":7');
});
test('static_deadline_stays_failure_after_cooperative_child_exit', async () => {
	const root = rootDirectory();
	const child = protocolDriver(
		root,
		`const fs=require('node:fs');const path=require('node:path');const root=process.argv[2];process.stdout.write('driver started\\n');process.stderr.write('driver stderr\\n');process.on('message',()=>{const marker=path.join(root,'capture-stop');if(fs.existsSync(marker)&&fs.readFileSync(marker,'utf8')==='stop\\n'){process.stdout.write('owned cleanup settled\\n');process.disconnect();}});process.send('ready');`,
	);
	await once(child.process, 'message');
	vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
	const waiting = waitStaticCapture(child, root, { work_seconds: 0.05, cleanup_seconds: 1 }).then(
		(exit) => `unexpected exit ${exit}`,
		(error: unknown) => String(error),
	);
	await vi.advanceTimersByTimeAsync(50);
	child.process.send('inspect-stop');
	expect(await waiting).toContain('static capture deadline');
	expect(await child.completion).toBe(0);
	expect(readFileSync(join(root, 'capture-stop'), 'utf8')).toBe('stop\n');
	expect(readFileSync(join(root, 'driver.stdout'), 'utf8')).toContain('owned cleanup settled');
	expect(readFileSync(join(root, 'driver.stderr'), 'utf8')).toBe('driver stderr\n');
	expect(readJson(join(root, 'driver-exit.json'))).toMatchObject({
		exit: 0,
		deadline_reached: true,
		driver_settled: true,
	});
});
test('static_deadline_reports_unsettled_child_without_signalling_it', async () => {
	const root = rootDirectory();
	const child = protocolDriver(
		root,
		`const fs=require('node:fs');const path=require('node:path');const root=process.argv[2];process.on('message',()=>{if(fs.readFileSync(path.join(root,'fixture-release'),'utf8')==='release\\n'){process.disconnect();process.exitCode=7;}});process.send('ready');`,
	);
	await once(child.process, 'message');
	vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
	const waiting = waitStaticCapture(child, root, {
		work_seconds: 0.03,
		cleanup_seconds: 0.03,
	}).then(
		(exit) => `unexpected exit ${exit}`,
		(error: unknown) => String(error),
	);
	await vi.advanceTimersByTimeAsync(30);
	await vi.advanceTimersByTimeAsync(30);
	expect(await waiting).toContain('cleanup unsettled');
	expect(child.process.exitCode).toBeNull();
	expect(readJson(join(root, 'driver-exit.json'))).toMatchObject({
		exit: null,
		driver_settled: false,
	});
	expect(readFileSync(join(root, 'capture-stop'), 'utf8')).toBe('stop\n');
	writeFileSync(join(root, 'fixture-release'), 'release\n');
	child.process.send('release');
	expect(await child.completion).toBe(7);
});
test.each([0, 7])(
	'static_wait_preserves_normal_success_and_failure_without_stop %s',
	async (exit) => {
		const root = rootDirectory();
		const child = protocolDriver(
			root,
			`process.on('message',()=>{process.exitCode=${exit};process.disconnect();});process.send('ready');`,
		);
		await once(child.process, 'message');
		vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
		const waiting = waitStaticCapture(child, root, { work_seconds: 2, cleanup_seconds: 1 });
		child.process.send('release');
		expect(await waiting).toBe(exit);
		expect(existsSync(join(root, 'capture-stop'))).toBe(false);
		expect(readJson(join(root, 'driver-exit.json'))).toMatchObject({
			exit,
			deadline_reached: false,
			driver_settled: true,
		});
	},
);
test('static_stop_refuses_to_overwrite_a_foreign_marker', async () => {
	const root = rootDirectory();
	writeFileSync(join(root, 'capture-stop'), 'different protocol\n');
	const child = protocolDriver(
		root,
		`process.send('ready');process.on('message',()=>{process.disconnect();});`,
	);
	await once(child.process, 'message');
	vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
	const waiting = waitStaticCapture(child, root, { work_seconds: 0.03, cleanup_seconds: 1 }).then(
		(exit) => `unexpected exit ${exit}`,
		(error: unknown) => String(error),
	);
	await vi.advanceTimersByTimeAsync(30);
	child.process.send('release');
	expect(await waiting).toContain('invalid private capture stop marker');
	expect(await child.completion).toBe(0);
	expect(readFileSync(join(root, 'capture-stop'), 'utf8')).toBe('different protocol\n');
	expect(readJson(join(root, 'driver-exit.json'))).toMatchObject({
		exit: 0,
		deadline_reached: true,
	});
});

test.each([false, true])(
	'discovery and oracle remain finite when a controlled descendant inherits stdout %s',
	async (oracle) => {
		const root = rootDirectory();
		const control = await oracleReleaseControl(root);
		let descendantPid: number | undefined;
		const executable = controlledExecutable(
			root,
			'inherited-output-parent',
			`const cp=require('node:child_process');const fs=require('node:fs');fs.watch(${JSON.stringify(root)},()=>{});process.stdout.write('partial inherited stdout\\n');process.stderr.write('partial inherited stderr\\n');const child=cp.spawn(process.execPath,['-e',${JSON.stringify(controlledOracleSocketSource(root, control.port, 0))},${JSON.stringify(root)}],{stdio:['ignore',1,2]});child.unref();`,
		);
		vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
		const task = oracle
			? runCapturedCommand({ command: [executable], repository: root, timeoutSeconds: 0.2 })
			: discoverCaptureTest([executable], root, 0.2);
		const observed = task.then(
			() => 'unexpected success',
			(error: unknown) => String(error),
		);
		try {
			try {
				descendantPid = await control.ready;
			} finally {
				// Cleanup must trigger the real parent's deadline even if readiness fails.
				await vi.advanceTimersByTimeAsync(200);
			}
			const failure = await observed;
			expect(failure).toContain(oracle ? 'command reap deadline' : 'capture discovery failed');
			expect(failure).toContain('"timeout":true');
			expect(failure).toContain(Buffer.from('partial inherited stdout\n').toString('base64'));
			expect(failure).toContain(Buffer.from('partial inherited stderr\n').toString('base64'));
		} finally {
			try {
				if (descendantPid !== undefined) {
					control.release(descendantPid);
					await control.waitForExit();
				}
			} finally {
				await control.close();
				await observed;
			}
		}
	},
);
function lateOracleSource(exit: number): string {
	return String.raw`
const fs=require('node:fs');const path=require('node:path');
const root=process.argv[2];const release=path.join(root,'oracle-release');
let watcher;let released=false;
const releaseIfPresent=()=>{
 if(released)return true;
 if(!fs.existsSync(release)||fs.readFileSync(release,'utf8')!=='release\n')return false;
 released=true;process.stdout.write('late observation');process.stderr.write('retained stderr');
 process.exitCode=${exit};watcher?.close();return true;
};
if(!releaseIfPresent()){
 watcher=fs.watch(root,releaseIfPresent);
 if(!releaseIfPresent()){const marker=path.join(root,'oracle-ready');fs.writeFileSync(marker+'.pending','ready\n');fs.renameSync(marker+'.pending',marker);}
}
`;
}

test.each([false, true])(
	'preexisting_release_finishes_controlled_oracle_before_watch (suppress notifications=%s)',
	async (suppressNotifications) => {
		const root = rootDirectory();
		writeFileSync(join(root, 'oracle-release'), 'release\n');
		// Suppression belongs only to this controlled subprocess; the file state stays real.
		const notificationControl = suppressNotifications
			? "require('node:fs').watch=()=>({close(){}});"
			: '';
		const executable = controlledExecutable(
			root,
			'pre-released-oracle',
			notificationControl + lateOracleSource(0),
		);
		const result = await runCapturedCommand({
			command: [executable, root],
			repository: root,
			timeoutSeconds: 5,
		});
		expect(result).toEqual({ exit: 0, stdout: 'late observation', stderr: 'retained stderr' });
	},
);

interface OracleReleaseControl {
	readonly port: number;
	readonly ready: Promise<number>;
	waitForExit(): Promise<void>;
	release(childPid: number | undefined): void;
	close(): Promise<void>;
}
/** Only the controlled child uses this loopback connection; command capture keeps real file descriptors. */
async function oracleReleaseControl(root: string): Promise<OracleReleaseControl> {
	let socket: Socket | null = null;
	let lines: Interface | null = null;
	let childPid: number | null = null;
	let childExited = false;
	let failure: Error | null = null;
	const readySchema = z.strictObject({
		kind: z.literal('ready'),
		root: z.literal(root),
		pid: z.number().int().positive(),
	});
	const server = createServer((connection) => {
		if (socket !== null) {
			connection.destroy();
			server.emit('error', new Error('oracle control received a second child'));
			return;
		}
		socket = connection;
		connection.once('close', () => {
			childExited = true;
			server.emit('oracle-exited');
		});
		connection.on('error', (error) => server.emit('error', error));
		lines = createInterface({ input: connection, crlfDelay: Infinity });
		lines.on('line', (line: string) => {
			try {
				if (childPid !== null) throw new Error('oracle repeated readiness');
				const ready = readySchema.parse(JSON.parse(line));
				childPid = ready.pid;
				server.emit('oracle-ready');
			} catch (error) {
				server.emit('error', error instanceof Error ? error : new Error(String(error)));
			}
		});
	});
	server.on('error', (error) => {
		failure ??= error;
	});
	const ready = once(server, 'oracle-ready', { signal: AbortSignal.timeout(5000) }).then(() => {
		if (childPid === null) throw new Error('oracle child identity absent');
		return childPid;
	});
	// Retain the rejection even if setup itself fails before the caller awaits it.
	void ready.catch(() => {});
	server.listen(0, '127.0.0.1');
	await once(server, 'listening');
	const address = server.address();
	if (address === null || typeof address === 'string')
		throw new Error('oracle control address absent');
	return {
		port: address.port,
		ready,
		waitForExit: async () => {
			if (failure !== null) throw failure;
			if (!childExited) await once(server, 'oracle-exited', { signal: AbortSignal.timeout(5000) });
		},
		release: (pid) => {
			if (failure !== null) throw failure;
			if (socket === null || childPid === null || pid !== childPid)
				throw new Error('oracle release must belong to the actual ready child');
			socket.write('release\n');
		},
		close: async () => {
			lines?.close();
			socket?.destroy();
			await new Promise<void>((resolve, reject) =>
				server.close((error) => (error ? reject(error) : resolve())),
			);
		},
	};
}
function controlledOracleSocketSource(root: string, port: number, exit: number): string {
	return [
		// The regression must remain valid even when file notifications are unavailable.
		"require('node:fs').watch=()=>{throw new Error('oracle lifecycle must not depend on file notifications');};",
		"const net=require('node:net');const readline=require('node:readline');",
		"const channel=net.createConnection({host:'127.0.0.1',port:" + port + '});',
		'const lines=readline.createInterface({input:channel,crlfDelay:Infinity});let released=false;',
		"lines.once('line',(line)=>{if(line!=='release')throw new Error('wrong oracle release');released=true;process.stdout.write('late observation');process.stderr.write('retained stderr');process.exit(" +
			exit +
			');});',
		"channel.on('connect',()=>channel.write(JSON.stringify({kind:'ready',root:" +
			JSON.stringify(root) +
			",pid:process.pid})+'\\n'));",
		"channel.on('error',()=>process.exit(1));",
		"channel.on('close',()=>process.exit(released ? " + exit + ' : 1));',
	].join('');
}
test.each([0, 1])(
	'oracle timeout remains failure after later exit %s when own-child signal is not delivered',
	async (exit) => {
		const root = rootDirectory();
		const control = await oracleReleaseControl(root);
		const executable = controlledExecutable(
			root,
			'late-oracle',
			controlledOracleSocketSource(root, control.port, exit),
		);
		// The real child reports readiness and cooperatively exits through its owned channel.
		// Only the parent's deadline is advanced; refusing SIGKILL remains observable.
		vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
		const refusedSignal = vi.spyOn(ChildProcess.prototype, 'kill').mockImplementation(function (
			this: ChildProcess,
		) {
			control.release(this.pid);
			return false;
		});
		const outcome = runCapturedCommand({
			command: [executable, root],
			repository: root,
			timeoutSeconds: 0.1,
		}).then(
			() => 'unexpected success',
			(error: unknown) => String(error),
		);
		try {
			await control.ready;
			await vi.advanceTimersByTimeAsync(100);
			const failure = await outcome;
			expect(failure).toContain('"exit":' + exit + ',"timeout":true');
			expect(failure).toContain(Buffer.from('late observation').toString('base64'));
			expect(failure).toContain(Buffer.from('retained stderr').toString('base64'));
			expect(refusedSignal).toHaveBeenCalledWith('SIGKILL');
		} finally {
			try {
				// A readiness failure must not leave the existing deadline frozen in cleanup.
				await vi.advanceTimersByTimeAsync(100);
			} finally {
				refusedSignal.mockRestore();
				await control.close();
				await outcome;
				vi.useRealTimers();
			}
		}
	},
);
