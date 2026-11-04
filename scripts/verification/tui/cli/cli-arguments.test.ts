import { spawnSync } from 'node:child_process';
import { existsSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { afterEach, expect, test } from 'vitest';

import { captureArtifact, sourceBinding } from '../evidence/source-binding.ts';
import {
	controlledExecutable,
	initializedSourceRepository,
} from '../fixtures/controlled-child-fixtures.ts';
import { protocolPlan, saveJson, temporaryRoot } from '../fixtures/receipt-fixtures.ts';
const roots: string[] = [];
afterEach(() => {
	for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
});
test('unknown capture option cannot launch the controlled driver', () => {
	const repository = initializedSourceRepository();
	roots.push(repository);
	writeFileSync(join(repository, 'crates/source.rs'), 'declared');
	const root = temporaryRoot();
	roots.push(root);
	const marker = join(root, 'unexpected-launch');
	const executable = controlledExecutable(
		root,
		'capture-protocol',
		`const fs=require('node:fs');const args=process.argv.slice(2);if(args.at(-1)==='--list')process.stdout.write(args[0]+': test\\n');else if(args[0]==='compact-sessions-all'){process.stdout.write(JSON.stringify({checkpoint:args[0],recognizedLines:[],recognizedFieldPairs:[],choiceFocusMeasurements:[],missingMarkers:['Sessions'],unexpectedMarkers:[]}));process.exitCode=1;}else {fs.writeFileSync(${JSON.stringify(marker)},'wrong launch');process.exitCode=7;}`,
	);
	const binding = captureArtifact(executable);
	const plan = {
		...protocolPlan(root),
		repository,
		source: sourceBinding(repository),
		capture_driver: binding,
		image_verifier: binding,
	};
	const path = join(root, 'plan.json');
	saveJson(path, plan);
	const result = spawnSync(
		process.execPath,
		[fileURLToPath(new URL('../capture/capture-run.ts', import.meta.url)), path, '--typo'],
		{ encoding: 'utf8', timeout: 5000 },
	);
	expect({
		status: result.status,
		launched: existsSync(marker),
		stderr: result.stderr,
	}).toMatchObject({ status: 2, launched: false });
});
test.each(['capture-run.ts', 'public-sessions-run.ts'])(
	'strict positional CLI rejects unknown missing and extra args before plan read %s',
	(entry) => {
		const entryPath =
			entry === 'capture-run.ts' ? 'capture/capture-run.ts' : 'sessions/public-sessions-run.ts';
		for (const args of [
			['unused.json', '--typo'],
			['unused.json', 'extra.json'],
			[],
			['--unknown'],
		]) {
			const result = spawnSync(
				process.execPath,
				[fileURLToPath(new URL(`../${entryPath}`, import.meta.url)), ...args],
				{ encoding: 'utf8', timeout: 5000 },
			);
			expect({ entry, args, status: result.status, stderr: result.stderr }).toMatchObject({
				status: 2,
			});
			expect(result.stderr).not.toContain('ENOENT');
		}
	},
);
test('strict evidence CLI rejects unknown trailing incompatible and missing values', () => {
	for (const args of [
		['--source', '.', '--typo'],
		['--gaps', 'extra'],
		['--source'],
		['--source', '.', '--gaps'],
		['--cua-plan', 'unused.json', '--surface'],
		['--cua-plan', 'unused.json', '--surface', 'bad', '--record', 'x'],
		[
			'--cua-plan',
			'unused.json',
			'--surface',
			'quota',
			'--record',
			'x',
			'--columns',
			'0',
			'--rows',
			'12',
		],
		['--baseline', 'x'],
	]) {
		const result = spawnSync(
			process.execPath,
			[fileURLToPath(new URL('../evidence/evidence-readback.ts', import.meta.url)), ...args],
			{ encoding: 'utf8', timeout: 5000 },
		);
		expect({ args, status: result.status, stderr: result.stderr }).toMatchObject({ status: 2 });
		expect(result.stderr).not.toContain('ENOENT');
	}
});
