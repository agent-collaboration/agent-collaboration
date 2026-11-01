import { expect, test } from 'vitest';

import { COMPONENT_SCENARIOS, COMPONENT_TEST } from '../../fixtures/component-fixture-contract.ts';
import { admitQuotaArgumentProfile, quotaArgumentAdmission } from './quota-argument-admission.ts';
import { quotaEffectArguments, quotaEffectScenarioSchema } from './quota-fixture-contract.ts';

function offlinePacket(): unknown {
	return {
		worktree: '/fixture',
		sourceHead: '4c66532915218899a6327864f6c2b43465db9401',
		sourceFrozen: false,
		nativeProductLaunchAuthorized: false,
		ownedSourceManifest: 'tmp/manifest.json',
		ownedSourceManifestSha256: '0'.repeat(64),
		sourceFilesSnapshot: 'tmp/snapshot',
		sourceSnapshotRetained: true,
		effectful: {
			artifact: null,
			package: 'codex-router-cli',
			features: ['quota-reset-test-harness'],
			argvSuffix: quotaEffectArguments,
			scenarioKeys: quotaEffectScenarioSchema.options,
			configurationEnvironment: 'QUOTA_EFFECT_FIXTURE_CONFIG',
			configurationIs: 'path to JSON file, not inline JSON',
			configurationFields: [
				'scenario',
				'outputDirectory',
				'events',
				'receipt',
				'controlSocket',
				'dispatchReceipt',
				'loginReceipt',
				'loginGateSocket',
				'loginStandin',
			],
			loginEnvironment: [
				'QUOTA_LOGIN_STANDIN_RECEIPT',
				'QUOTA_LOGIN_STANDIN_GATE',
				'QUOTA_LOGIN_STANDIN_EXIT',
				'PATH',
			],
			loginExit: { 'login-succeeded': 0, 'login-cancelled130': 130, 'login-failed7': 7 },
		},
		readOnly: {
			artifact: null,
			package: 'agent-router-quota-tui',
			argvSuffix: ['--exact', COMPONENT_TEST, '--ignored', '--nocapture', '--test-threads=1'],
			scenarioKeys: Object.keys(COMPONENT_SCENARIOS),
			scenarioEnvironment: 'QUOTA_COMPONENT_FIXTURE',
			optionalReceiptEnvironment: [
				'QUOTA_COMPONENT_FIXTURE_RECEIPT_FILE',
				'QUOTA_COMPONENT_FIXTURE_DISPATCH_RECEIPT_FILE',
			],
			releaseEnvironment: 'QUOTA_COMPONENT_FIXTURE_RELEASE_FILE',
			releaseRequiredFor: [
				'inspection-pending',
				'commit-pending',
				'revalidation-pending',
				'refresh-pending',
				'save-pending',
			],
			releaseBytes: 'settle\n',
		},
		commonChildEnvironment: { NO_COLOR: 'absent', TMPDIR: '/private/tmp' },
		r19: { status: 'unbound placeholder; no grouped projection or persisted reopen' },
	};
}
test('source admission counts both refresh children and both protocols without an executable pass', () => {
	expect(quotaArgumentAdmission(offlinePacket())).toMatchObject({
		sourceProfiles: 58,
		executableChecks: 0,
		productLaunches: 0,
		productPasses: 0,
	});
});
test.each([
	{ argumentsValue: ['--sessions-picker'] },
	{ argumentsValue: ['--exact', COMPONENT_TEST, '--ignored', '--nocapture', '--test-threads=1'] },
	{ argumentsValue: [...quotaEffectArguments, '--include-ignored'] },
])('effectful profile rejects a different entry or extra flag: %j', ({ argumentsValue }) => {
	expect(() =>
		admitQuotaArgumentProfile({
			kind: 'effectful',
			scenario: 'captured-actions',
			arguments: argumentsValue,
		}),
	).toThrow('exact declared entry');
});
test('read-only admission cannot select a mutating scenario', () => {
	expect(() =>
		admitQuotaArgumentProfile({
			kind: 'read-only',
			scenario: 'remove-middle',
			arguments: ['--exact', COMPONENT_TEST, '--ignored', '--nocapture', '--test-threads=1'],
		}),
	).toThrow('unknown read-only');
});
test('an executable object cannot turn this source packet into a frozen declaration', () => {
	const packet = offlinePacket();
	if (typeof packet !== 'object' || packet === null) throw new Error('packet absent');
	expect(() => quotaArgumentAdmission({ ...packet, sourceFrozen: true })).toThrow();
});
