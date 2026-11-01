/** These are launch-admission probes over harmless files; no TUI or login child runs. */
import {
	chmodSync,
	existsSync,
	mkdirSync,
	mkdtempSync,
	realpathSync,
	rmSync,
	symlinkSync,
	writeFileSync,
} from 'node:fs';
import { join } from 'node:path';

import { afterEach, expect, test } from 'vitest';

import { fileDigest } from '../evidence/source-binding.ts';
import {
	quotaEffectScenarioSchema,
	quotaLoginExit,
	quotaRequiresPrivateGuard,
} from '../journeys/flows/quota-fixture-contract.ts';
import type { QuotaEffectScenario } from '../journeys/flows/quota-fixture-contract.ts';
import { quotaEffectLaunch } from './quota-effect-launch.ts';
import { shortTemporaryRoot } from './short-temporary-root.ts';

const roots: string[] = [];
afterEach(() => {
	for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
});

function launchRecord(scenario: QuotaEffectScenario): Readonly<Record<string, string>> {
	const output = realpathSync(mkdtempSync(join(shortTemporaryRoot(), '2026-11-qb-')));
	roots.push(output);
	const binary = join(output, 'declared-libtest');
	writeFileSync(binary, 'not executed by this test');
	chmodSync(binary, 0o700);
	const effectConfiguration = join(output, 'effect.json');
	const effect = {
		scenario,
		outputDirectory: output,
		events: join(output, 'events'),
		receipt: join(output, 'receipt'),
		controlSocket: join(output, 'control'),
		dispatchReceipt: join(output, 'dispatch'),
	};
	const record: Record<string, string> = {
		surface: 'quota-effects',
		binary,
		binary_sha256: fileDigest(binary),
		approved_cwd: output,
		output,
		effect_configuration: effectConfiguration,
		effect_configuration_sha256: '',
	};
	if (!quotaRequiresPrivateGuard(scenario))
		writeFileSync(effectConfiguration, JSON.stringify(effect));
	else {
		const standin = join(output, 'quota-login-recording-standin');
		writeFileSync(standin, 'not executed by this test');
		chmodSync(standin, 0o700);
		const prefix = join(output, 'bin');
		mkdirSync(prefix, { mode: 0o700 });
		symlinkSync(standin, join(prefix, 'codex-router'));
		writeFileSync(
			effectConfiguration,
			JSON.stringify({
				...effect,
				loginReceipt: join(output, 'login'),
				loginGateSocket: join(output, 'login-gate'),
				loginStandin: standin,
			}),
		);
		record['login_standin_sha256'] = fileDigest(standin);
		record['login_path_prefix'] = prefix;
	}
	record['effect_configuration_sha256'] = fileDigest(effectConfiguration);
	return record;
}

test.each(quotaEffectScenarioSchema.options)(
	'source configuration %s admits no executable side effect',
	(scenario) => {
		const record = launchRecord(scenario),
			launch = quotaEffectLaunch(record);
		expect(launch.executable).toBe(record['binary']);
		expect(launch.arguments).toEqual([
			'--exact',
			'quota::quota_account_adapter::tests::effectful_fixture::compiled_quota_effectful_fixture',
			'--ignored',
			'--nocapture',
			'--test-threads=1',
		]);
		expect(launch.environment['QUOTA_LOGIN_STANDIN_EXIT']).toBe(
			quotaRequiresPrivateGuard(scenario) ? String(quotaLoginExit(scenario) ?? 0) : undefined,
		);
		expect(launch.environment['HOME']).toBe(process.env['HOME']);
		for (const name of [
			'events',
			'receipt',
			'control',
			'dispatch',
			'login',
			'login-gate',
			'quota-child.pid',
		])
			expect(existsSync(join(record['output'] ?? '', name))).toBe(false);
	},
);
test('unqualified or replaced product bytes are refused before a launch', () => {
	const record = launchRecord('captured-actions');
	expect(() => quotaEffectLaunch({ ...record, binary_sha256: '0'.repeat(64) })).toThrow(
		'artifact changed',
	);
});
test('login PATH cannot resolve to another executable', () => {
	const record = launchRecord('login-failed7');
	const prefix = record['login_path_prefix'];
	if (prefix === undefined) throw new Error('login prefix missing');
	rmSync(join(prefix, 'codex-router'));
	symlinkSync('/bin/echo', join(prefix, 'codex-router'));
	expect(() => quotaEffectLaunch(record)).toThrow('PATH must resolve exactly');
});
test('public output permissions and preexisting outputs are rejected', () => {
	const record = launchRecord('remove-held'),
		root = record['output'];
	if (root === undefined) throw new Error('output missing');
	chmodSync(root, 0o755);
	expect(() => quotaEffectLaunch(record)).toThrow('private mode 0700');
	chmodSync(root, 0o700);
	writeFileSync(join(root, 'receipt'), 'previous run');
	expect(() => quotaEffectLaunch(record)).toThrow('output already exists');
});
