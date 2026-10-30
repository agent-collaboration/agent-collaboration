import {
	mkdirSync,
	mkdtempSync,
	realpathSync,
	readFileSync,
	rmSync,
	symlinkSync,
	writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { afterEach, expect, test, vi } from 'vitest';

import { childLaunchConfiguration } from './fixture-child.ts';
import { PtyGridDriver } from './pty-grid-driver.ts';
import type { PtyGridTarget } from './pty-grid-driver.ts';

const roots: string[] = [];
function ownedRoot(): string {
	const root = realpathSync(mkdtempSync(join(tmpdir(), 'sessions-receipts-')));
	roots.push(root);
	return root;
}
afterEach(() => {
	vi.unstubAllEnvs();
	for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
});

test('sessions receipt input reaches only the fixture and absence stays absent', () => {
	const output = ownedRoot();
	const directory = join(output, 'session-receipts');
	mkdirSync(directory);
	const configuration = {
		surface: 'sessions',
		binary: '/declared/fixture',
		output,
		temporary_directory: output,
		capability: 'fixture-capability',
	};
	expect(childLaunchConfiguration(configuration).environment).not.toHaveProperty(
		'TUI_SESSIONS_FIXTURE_RECEIPT_DIR',
	);
	expect(
		childLaunchConfiguration(configuration, { TUI_SESSIONS_FIXTURE_RECEIPT_DIR: directory })
			.environment,
	).toHaveProperty('TUI_SESSIONS_FIXTURE_RECEIPT_DIR', directory);
});

test('sessions receipt input rejects missing, reused, linked and outside directories', () => {
	const output = ownedRoot();
	const outside = ownedRoot();
	const full = join(output, 'full');
	mkdirSync(full);
	writeFileSync(join(full, 'picker-outcome.json'), '{}');
	const linked = join(output, 'linked');
	symlinkSync(outside, linked);
	const configuration = {
		surface: 'sessions',
		binary: '/declared/fixture',
		output,
		temporary_directory: output,
		capability: 'fixture-capability',
	};
	for (const directory of [
		'',
		'relative',
		join(output, 'missing'),
		full,
		linked,
		outside,
		output,
	]) {
		expect(() =>
			childLaunchConfiguration(configuration, { TUI_SESSIONS_FIXTURE_RECEIPT_DIR: directory }),
		).toThrow();
	}
});

test('quota fixture never receives the sessions receipt input', () => {
	const output = ownedRoot();
	const configuration = {
		surface: 'quota',
		binary: '/declared/fixture',
		output,
		temporary_directory: output,
		capability: 'fixture-capability',
		root: output,
		provider: '127.0.0.1:1',
	};
	expect(
		childLaunchConfiguration(configuration, { TUI_SESSIONS_FIXTURE_RECEIPT_DIR: '/unrelated' })
			.environment,
	).not.toHaveProperty('TUI_SESSIONS_FIXTURE_RECEIPT_DIR');
});

// These children observe only the transport environment, never product UI or effects.
test('PtyGridDriver forwards only its explicitly nominated sessions receipt directory', async () => {
	vi.stubEnv('TUI_SESSIONS_FIXTURE_RECEIPT_DIR', '/ambient/must-not-flow');
	const output = ownedRoot();
	const directory = join(output, 'session-receipts');
	mkdirSync(directory);
	const bridge = join(output, 'controlled-bridge.mjs');
	writeFileSync(
		bridge,
		`#!${process.execPath}\nimport {writeFileSync} from 'node:fs';\nwriteFileSync(${JSON.stringify(join(output, 'observed-directory'))}, process.env.TUI_SESSIONS_FIXTURE_RECEIPT_DIR ?? 'absent');\nprocess.stdout.write('TUI_GRID READY 120 36\\n');\nprocess.stdin.on('data', () => {process.stdout.write('TUI_GRID STOPPED 0\\n');process.exit(0);});\n`,
		{ mode: 0o700 },
	);
	await [undefined, directory].reduce<Promise<void>>(async (previous, nominated) => {
		await previous;
		const target = {
			repository: output,
			driver: bridge,
			surface: 'sessions',
			keys: 'legacy',
			evidence: output,
			...(nominated === undefined ? {} : { sessionsReceiptDirectory: nominated }),
		} satisfies PtyGridTarget;
		const driver = await PtyGridDriver.start(target);
		expect(await driver.stop()).toBe(0);
		expect(readFileSync(join(output, 'observed-directory'), 'utf8')).toBe(nominated ?? 'absent');
	}, Promise.resolve());
});

// Each variant uses the same driver environment route; these are transport facts only.
test.each(['wide-glyph-left-border', 'source-qualified-outcome', 'configured-source-new'] as const)(
	'PtyGridDriver forwards the explicit storyboard variant %s',
	async (storyboardVariant) => {
		const output = ownedRoot();
		const bridge = join(output, 'variant-bridge.mjs');
		writeFileSync(
			bridge,
			`#!${process.execPath}\nimport { writeFileSync } from 'node:fs';\nwriteFileSync(${JSON.stringify(join(output, 'variant'))}, process.env.TUI_GRID_STORYBOARD_VARIANT ?? 'absent');\nprocess.stdout.write('TUI_GRID READY 120 36\\n');\nprocess.stdin.on('data', () => { process.stdout.write('TUI_GRID STOPPED 0\\n'); process.exit(0); });\n`,
			{ mode: 0o700 },
		);
		const driver = await PtyGridDriver.start({
			repository: output,
			driver: bridge,
			surface: 'sessions-filters',
			keys: 'legacy',
			evidence: output,
			storyboardVariant,
		});
		expect(await driver.stop()).toBe(0);
		expect(readFileSync(join(output, 'variant'), 'utf8')).toBe(storyboardVariant);
	},
);
