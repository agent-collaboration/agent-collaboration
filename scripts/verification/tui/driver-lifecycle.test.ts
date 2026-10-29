import { ChildProcess } from 'node:child_process';
import { once } from 'node:events';
import {
	closeSync,
	openSync,
	readFileSync,
	rmSync,
	writeFileSync,
	existsSync,
	watch,
} from 'node:fs';
import { join } from 'node:path';

import { afterEach, expect, test, vi } from 'vitest';

import { checkDriver } from './capture-execution.ts';
import { controlledExecutable, initializedSourceRepository } from './controlled-child-fixtures.ts';
import {
	discoverCaptureTest,
	runCapturedCommand,
	startDriver,
	waitStaticCapture,
} from './driver-lifecycle.ts';
import type { DriverChild } from './driver-lifecycle.ts';
import { protocolPlan, temporaryRoot } from './receipt-fixtures.ts';
import { captureArtifact, readJson, sourceBinding } from './source-binding.ts';
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
function waitForMarker(root: string, name: string, expected: string): Promise<void> {
	const marker = join(root, name);
	const watcher = watch(root, { signal: AbortSignal.timeout(5000) });
	return new Promise((resolve, reject) => {
		const observe = (): void => {
			if (existsSync(marker) && readFileSync(marker, 'utf8') === expected) {
				watcher.close();
				resolve();
			}
		};
		watcher.on('change', observe);
		watcher.once('error', reject);
		watcher.once('close', () => {
			if (!existsSync(marker) || readFileSync(marker, 'utf8') !== expected)
				reject(new Error(`child did not publish ${name}`));
		});
		observe();
	});
}
test('cua_fixture_discovery_rejects_more_than_one_libtest_filter', async () => {
	const repository = initializedSourceRepository();
	roots.push(repository);
	writeFileSync(join(repository, 'crates/input.rs'), 'source');
	const executable = controlledExecutable(
		repository,
		'exact-protocol',
		`const args=process.argv.slice(2);if(args.at(-1)==='--list'){const filters=args.filter(value=>!value.startsWith('--'));if(filters.length!==1)process.exit(2);process.stdout.write(filters[0]+': test\\n1 test, 0 benchmarks');}else if(args[0]==='compact-quota-all'){process.stdout.write(JSON.stringify({checkpoint:args[0],recognizedLines:[],recognizedFieldPairs:[],choiceFocusMeasurements:[],missingMarkers:['Quota status'],unexpectedMarkers:[]}));process.exitCode=1;}else process.exitCode=2;`,
	);
	const binding = captureArtifact(executable);
	const plan = {
		...protocolPlan(rootDirectory()),
		repository,
		source: sourceBinding(repository),
		capture_driver: binding,
		harness_binary: binding,
		image_verifier: binding,
	};
	await checkDriver(plan, 'cua-quota', 'normal');
	expect(existsSync(plan.output)).toBe(false);
});
test('capture_discovery_bounds_and_reaps_its_exact_child', async () => {
	const root = rootDirectory();
	const executable = controlledExecutable(
		root,
		'held-discovery',
		`const fs=require('node:fs');fs.watch(process.argv[2],()=>{});process.stdout.write('partial list\\n');process.stderr.write('partial error\\n');fs.writeFileSync(process.argv[2]+'/discovery-ready','ready\\n');`,
	);
	const ready = waitForMarker(root, 'discovery-ready', 'ready\n');
	// The child must emit real output before this unit advances the deadline.
	// Its process, raw descriptors and exact-child SIGKILL remain real.
	vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
	const discovery = discoverCaptureTest([executable, root], root, 0.2).then(
		() => '',
		(error: unknown) => String(error),
	);
	let failure: string;
	try {
		await ready;
		expect(readFileSync(join(root, 'discovery-ready'), 'utf8')).toBe('ready\n');
	} finally {
		try {
			await vi.advanceTimersByTimeAsync(200);
			failure = await discovery;
		} finally {
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
		`const fs=require('node:fs');const path=require('node:path');const root=process.argv[2];process.stdout.write('driver started\\n');process.stderr.write('driver stderr\\n');const watcher=fs.watch(root,()=>{const marker=path.join(root,'capture-stop');if(fs.existsSync(marker)&&fs.readFileSync(marker,'utf8')==='stop\\n'){process.stdout.write('owned cleanup settled\\n');watcher.close();process.disconnect();}});process.send('ready');`,
	);
	await once(child.process, 'message');
	vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
	const waiting = expect(
		waitStaticCapture(child, root, { work_seconds: 0.05, cleanup_seconds: 1 }).then((exit) => {
			throw new Error(
				`unexpected exit ${exit}; ${readFileSync(join(root, 'driver.stderr'), 'utf8')}`,
			);
		}),
	).rejects.toThrow('static capture deadline');
	await vi.advanceTimersByTimeAsync(50);
	await waiting;
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
		`const fs=require('node:fs');const path=require('node:path');const root=process.argv[2];const watcher=fs.watch(root,()=>{if(fs.existsSync(path.join(root,'fixture-release'))){watcher.close();process.disconnect();process.exitCode=7;}});process.send('ready');`,
	);
	await once(child.process, 'message');
	vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
	const waiting = expect(
		waitStaticCapture(child, root, { work_seconds: 0.03, cleanup_seconds: 0.03 }).then((exit) => {
			throw new Error(
				`unexpected exit ${exit}; ${readFileSync(join(root, 'driver.stderr'), 'utf8')}`,
			);
		}),
	).rejects.toThrow('cleanup unsettled');
	await vi.advanceTimersByTimeAsync(60);
	await waiting;
	expect(child.process.exitCode).toBeNull();
	expect(readJson(join(root, 'driver-exit.json'))).toMatchObject({
		exit: null,
		driver_settled: false,
	});
	expect(readFileSync(join(root, 'capture-stop'), 'utf8')).toBe('stop\n');
	writeFileSync(join(root, 'fixture-release'), 'release\n');
	expect(await child.completion).toBe(7);
});
test.each([0, 7])(
	'static_wait_preserves_normal_success_and_failure_without_stop %s',
	async (exit) => {
		const root = rootDirectory();
		const child = protocolDriver(root, `process.exitCode=${exit};`);
		expect(await waitStaticCapture(child, root, { work_seconds: 2, cleanup_seconds: 1 })).toBe(
			exit,
		);
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
	const waiting = waitStaticCapture(child, root, { work_seconds: 0.03, cleanup_seconds: 1 });
	const rejected = expect(waiting).rejects.toThrow('invalid private capture stop marker');
	await vi.advanceTimersByTimeAsync(30);
	child.process.send('release');
	await rejected;
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
		const release = join(root, 'descendant-release');
		const executable = controlledExecutable(
			root,
			'inherited-output-parent',
			`const cp=require('node:child_process');const fs=require('node:fs');fs.watch(${JSON.stringify(root)},()=>{});const child=cp.spawn(process.execPath,['-e',${JSON.stringify("const fs=require('node:fs');const path=require('node:path');const root=process.argv[1];const watcher=fs.watch(root,()=>{if(fs.existsSync(path.join(root,'descendant-release'))){watcher.close();}});fs.writeFileSync(path.join(root,'descendant-ready'),'ready');")},${JSON.stringify(root)}],{stdio:['ignore',1,2]});child.unref();process.stdout.write('partial inherited stdout\\n');process.stderr.write('partial inherited stderr\\n');`,
		);
		const ready = waitForMarker(root, 'descendant-ready', 'ready');
		vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
		const task = oracle
			? runCapturedCommand({ command: [executable], repository: root, timeoutSeconds: 0.2 })
			: discoverCaptureTest([executable], root, 0.2);
		let settled = false;
		const observed = task.then(
			(result) => {
				settled = true;
				return { kind: 'settled', result };
			},
			(error: unknown) => {
				settled = true;
				return { kind: 'settled', error: String(error) };
			},
		);
		let timer: NodeJS.Timeout | undefined;
		try {
			await ready;
			await vi.advanceTimersByTimeAsync(200);
			vi.useRealTimers();
			const outcome = await Promise.race([
				observed,
				new Promise<{ kind: string }>((resolve) => {
					timer = setTimeout(() => resolve({ kind: 'deadline' }), 2800);
				}),
			]);
			expect(outcome.kind).toBe('settled');
			expect(settled).toBe(true);
		} finally {
			if (timer !== undefined) clearTimeout(timer);
			writeFileSync(release, 'release\n');
			await observed;
		}
	},
);
test.each([0, 1])(
	'oracle timeout remains failure after later exit %s when own-child signal is not delivered',
	async (exit) => {
		const root = rootDirectory();
		const executable = controlledExecutable(
			root,
			'late-oracle',
			`const fs=require('node:fs');const root=${JSON.stringify(root)};const watcher=fs.watch(root,()=>{if(fs.existsSync(root+'/oracle-release')){watcher.close();process.stdout.write('late observation');process.stderr.write('retained stderr');process.exitCode=${exit};}});fs.writeFileSync(root+'/oracle-ready','ready');`,
		);
		const ready = waitForMarker(root, 'oracle-ready', 'ready');
		vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
		const refusedSignal = vi.spyOn(ChildProcess.prototype, 'kill').mockReturnValue(false);
		try {
			const waiting = expect(
				runCapturedCommand({ command: [executable], repository: root, timeoutSeconds: 0.1 }),
			).rejects.toThrow(`"exit":${exit},"timeout":true`);
			await ready;
			await vi.advanceTimersByTimeAsync(100);
			writeFileSync(join(root, 'oracle-release'), 'release');
			await waiting;
			expect(refusedSignal).toHaveBeenCalledWith('SIGKILL');
		} finally {
			refusedSignal.mockRestore();
		}
	},
);
