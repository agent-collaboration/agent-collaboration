import { spawn } from 'node:child_process';
/** Fixed wrapper contract qualification; this test never claims a component outcome. */
import { createHash } from 'node:crypto';
import {
	existsSync,
	readFileSync,
	readdirSync,
	mkdirSync,
	mkdtempSync,
	realpathSync,
	rmSync,
	symlinkSync,
	writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { afterEach, expect, test } from 'vitest';

import { childLaunchConfiguration } from '../fixtures/fixture-child.ts';
const roots: string[] = [];
afterEach(() => {
	for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
});

test('the real wrapper handshake stays live until stopped and never spawns the example', async () => {
	const output = realpathSync(mkdtempSync(join(tmpdir(), 'showcase-handshake-')));
	roots.push(output);
	const binary = join(output, 'component_showcase');
	const bytes = 'this file must never execute';
	writeFileSync(binary, bytes, { mode: 0o700 });
	const receipts = join(output, 'receipts');
	mkdirSync(receipts);
	const configuration = join(output, 'fixture-command.json');
	writeFileSync(
		configuration,
		JSON.stringify({
			surface: 'showcase',
			binary,
			binary_sha256: createHash('sha256').update(bytes).digest('hex'),
			output,
			temporary_directory: output,
			approved_cwd: output,
			receipt_directory: receipts,
		}),
	);
	const child = spawn(
		process.execPath,
		[fileURLToPath(new URL('../fixtures/fixture-child.ts', import.meta.url)), configuration],
		{
			env: {
				...process.env,
				QUOTA_STORYBOARD_FIXTURE_COMMAND: configuration,
				TUI_GRID_HANDSHAKE_ONLY: '1',
			},
			stdio: 'pipe',
		},
	);
	const completed = new Promise<number | null>((resolve, reject) => {
		child.once('close', resolve);
		child.once('error', reject);
	});
	let stdout = '';
	const ready = new Promise<void>((resolve) =>
		child.stdout.on('data', (chunk: Buffer) => {
			stdout += chunk.toString('utf8');
			if (stdout.includes('TUI_SHOWCASE_WRAPPER_HANDSHAKE_READY')) resolve();
		}),
	);
	try {
		await Promise.race([
			ready,
			completed.then((code) => {
				throw new Error('wrapper exited before handshake: ' + code);
			}),
		]);
		expect(child.exitCode).toBeNull();
		expect(existsSync(join(output, 'quota-wrapper.identity'))).toBe(true);
		expect(existsSync(join(output, 'quota-child.pid'))).toBe(false);
		child.kill('SIGTERM');
		expect(await completed).toBe(0);
		expect(readFileSync(join(output, 'wrapper-handshake.exit'), 'utf8')).toBe('0');
		expect(existsSync(join(output, 'quota-child.pid'))).toBe(false);
		expect(readdirSync(receipts)).toEqual([]);
	} finally {
		if (child.exitCode === null && child.signalCode === null) {
			child.kill('SIGTERM');
			await completed;
		}
	}
});
test('showcase has an exact local argv/hash/owned receipts contract, independent of Host fields', () => {
	const output = realpathSync(mkdtempSync(join(tmpdir(), 'showcase-launch-')));
	roots.push(output);
	const binary = join(output, 'component_showcase');
	const bytes = 'controlled fixture identity';
	writeFileSync(binary, bytes, { mode: 0o700 });
	const receipts = join(output, 'receipts');
	mkdirSync(receipts);
	const configuration = {
		surface: 'showcase',
		binary,
		binary_sha256: createHash('sha256').update(bytes).digest('hex'),
		output,
		temporary_directory: output,
		approved_cwd: output,
		receipt_directory: receipts,
	};
	const launched = childLaunchConfiguration(configuration);
	expect(launched).toMatchObject({
		executable: binary,
		arguments: ['--receipt-directory', receipts],
		cwd: output,
	});
	expect(launched.environment['HOME']).toBe(process.env['HOME']);
	expect(launched.environment['TERM']).toBe('xterm-256color');
	expect(launched.environment).not.toHaveProperty('TUI_SESSIONS_FIXTURE_RECEIPT_DIR');
	for (const change of [
		{ binary_sha256: '0'.repeat(64) },
		{ argv: '--arbitrary' },
		{ expected_service_id: 'forbidden' },
		{ receipt_directory: output },
		{ approved_cwd: 'relative' },
	])
		expect(() => childLaunchConfiguration({ ...configuration, ...change })).toThrow();
	const linked = join(output, 'linked');
	symlinkSync(binary, linked);
	expect(() => childLaunchConfiguration({ ...configuration, binary: linked })).toThrow();
	writeFileSync(join(receipts, 'prior'), 'reused');
	expect(() => childLaunchConfiguration(configuration)).toThrow();
});
