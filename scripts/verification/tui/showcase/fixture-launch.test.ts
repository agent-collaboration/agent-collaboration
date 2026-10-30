/** Fixed wrapper contract qualification; this test never claims a component outcome. */
import { createHash } from 'node:crypto';
import { mkdirSync, mkdtempSync, realpathSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { afterEach, expect, test } from 'vitest';

import { childLaunchConfiguration } from '../fixture-child.ts';
const roots: string[] = [];
afterEach(() => {
	for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
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
