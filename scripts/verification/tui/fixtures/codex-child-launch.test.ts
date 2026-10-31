import { writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { expect, test } from 'vitest';

import { codexLaunchRecord } from '../codex-acp/codex-acp-target.ts';
import { targetFixture } from '../codex-acp/codex-acp-test-fixtures.ts';
import { codexChildLaunch } from './codex-child-launch.ts';

async function launchConfiguration(): Promise<{
	readonly root: string;
	readonly configuration: Record<string, string>;
	readonly expected: ReturnType<typeof codexLaunchRecord>;
}> {
	const { root, target } = await targetFixture();
	return {
		root,
		configuration: {
			surface: 'codex-acp',
			binary: target.codex.path,
			binary_sha256: target.codex.sha256,
			target: JSON.stringify(target),
			output: join(root, 'output'),
		},
		expected: codexLaunchRecord(target),
	};
}

test('the codex-acp child is the qualified target argv with the inherited normal environment', async () => {
	const { configuration, expected } = await launchConfiguration();
	const launch = codexChildLaunch(configuration);
	expect(launch.executable).toBe(expected.program);
	expect(launch.arguments).toEqual(expected.arguments);
	expect(launch.cwd).toBe(expected.cwd);
	// Normal Codex home is inherited, never replaced by a temporary HOME.
	expect(launch.environment['HOME']).toBe(process.env['HOME']);
	expect(launch.environment['CODEX_HOME']).toBe(process.env['CODEX_HOME']);
});

test('extra, missing or mismatched launch fields are refused before any spawn', async () => {
	const { configuration } = await launchConfiguration();
	expect(() => codexChildLaunch({ ...configuration, approved_cwd: '/elsewhere' })).toThrow(
		'exact bound configuration fields',
	);
	const { output: _output, ...missing } = configuration;
	expect(() => codexChildLaunch(missing)).toThrow('exact bound configuration fields');
	expect(() => codexChildLaunch({ ...configuration, binary_sha256: '0'.repeat(64) })).toThrow(
		'binary binding differs from its target',
	);
	expect(() => codexChildLaunch({ ...configuration, target: '{}' })).toThrow();
});

test('the launch re-qualifies the target at spawn time', async () => {
	const { root, configuration } = await launchConfiguration();
	// The installed binary changed between planning and launch.
	writeFileSync(join(root, 'codex'), '#!/usr/bin/env node\n');
	expect(() => codexChildLaunch(configuration)).toThrow('artifact changed');
});
