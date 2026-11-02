import { once } from 'node:events';
import {
	closeSync,
	existsSync,
	mkdirSync,
	openSync,
	readFileSync,
	rmSync,
	writeFileSync,
} from 'node:fs';
import { join } from 'node:path';

import { afterEach, expect, test, vi } from 'vitest';

import { readJson } from '../evidence/source-binding.ts';
import { controlledExecutable } from '../fixtures/controlled-child-fixtures.ts';
import { temporaryRoot } from '../fixtures/receipt-fixtures.ts';
import { startDriver, waitStaticCapture } from '../processes/driver-lifecycle.ts';
import type { DriverChild } from '../processes/driver-lifecycle.ts';
import { waitPublicDriver } from './public-lifecycle.ts';
const roots: string[] = [];
const children: DriverChild[] = [];
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
function fixture(
	cooperative: boolean,
	preexistingStop = false,
): { root: string; child: DriverChild } {
	const root = temporaryRoot();
	roots.push(root);
	mkdirSync(join(root, 'cua-sessions'));
	mkdirSync(join(root, 'cua-quota'));
	const marker = cooperative ? 'cua-sessions/cua-stop' : 'fixture-release';
	if (preexistingStop) writeFileSync(join(root, marker), 'stop\n');
	const executable = controlledExecutable(
		root,
		'owned-cua-driver',
		`const fs=require('node:fs');const path=require('node:path');const root=process.argv[2];const stop=path.join(root,${JSON.stringify(marker)});fs.watch=()=>{throw new Error('CUA lifecycle must use owned messages');};let finished=false;const finishIfStopped=()=>{if(finished)return true;if(!fs.existsSync(stop))return false;finished=true;process.disconnect();return true;};if(!finishIfStopped()){process.on('message',finishIfStopped);process.send('ready');}`,
	);
	const stdout = openSync(join(root, 'stdout'), 'wx');
	const stderr = openSync(join(root, 'stderr'), 'wx');
	try {
		const child = startDriver([executable, root], { stdio: ['ignore', stdout, stderr, 'ipc'] });
		children.push(child);
		return { root, child };
	} finally {
		closeSync(stdout);
		closeSync(stderr);
	}
}
test('public timeout requests its own CUA stop and remains failure after cooperative zero exit', async () => {
	const { root, child } = fixture(true);
	await once(child.process, 'message');
	vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
	const waiting = expect(
		waitPublicDriver(child, root, { workSeconds: 0.03, cleanupSeconds: 1 }),
	).rejects.toThrow('public CUA failed');
	await vi.advanceTimersByTimeAsync(30);
	child.process.send('inspect-stop');
	expect(await child.completion).toBe(0);
	await waiting;
	expect(readFileSync(join(root, 'cua-sessions/cua-stop'), 'utf8')).toBe('stop\n');
	expect(existsSync(join(root, 'capture-stop'))).toBe(false);
	expect(readJson(join(root, 'driver-exit.json'))).toMatchObject({
		exit: 0,
		deadline_reached: true,
		driver_settled: true,
	});
});
test('public timeout preserves unsettled exact child without signaling', async () => {
	const { root, child } = fixture(false);
	await once(child.process, 'message');
	vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
	const waiting = expect(
		waitPublicDriver(child, root, { workSeconds: 0.03, cleanupSeconds: 0.03 }),
	).rejects.toThrow('cleanup unsettled');
	await vi.advanceTimersByTimeAsync(30);
	await vi.advanceTimersByTimeAsync(30);
	await waiting;
	expect(child.process.exitCode).toBeNull();
	expect(readJson(join(root, 'driver-exit.json'))).toMatchObject({
		exit: null,
		driver_settled: false,
	});
	writeFileSync(join(root, 'fixture-release'), 'release');
	child.process.send('inspect-stop');
	expect(await child.completion).toBe(0);
});
test('fixture CUA timeout uses declared CUA marker instead of static capture marker', async () => {
	const { root, child } = fixture(true);
	await once(child.process, 'message');
	vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
	const waiting = expect(
		waitStaticCapture(
			child,
			root,
			{ work_seconds: 0.03, cleanup_seconds: 1 },
			join(root, 'cua-sessions/cua-stop'),
		),
	).rejects.toThrow('static capture deadline');
	await vi.advanceTimersByTimeAsync(30);
	child.process.send('inspect-stop');
	expect(await child.completion).toBe(0);
	await waiting;
	expect(existsSync(join(root, 'capture-stop'))).toBe(false);
});

test.each([false, true])(
	'preexisting CUA release settles without installing any watcher (cooperative=%s)',
	async (cooperative) => {
		const { child } = fixture(cooperative, true);
		expect(await child.completion).toBe(0);
	},
);
