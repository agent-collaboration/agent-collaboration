/** Admission/staging controls use harmless executable files and never launch them. */
import {
	chmodSync,
	existsSync,
	mkdirSync,
	mkdtempSync,
	readFileSync,
	realpathSync,
	rmSync,
	writeFileSync,
} from 'node:fs';
import { join } from 'node:path';

import { afterEach, expect, test } from 'vitest';

import { captureArtifact } from '../../evidence/source-binding.ts';
import { quotaEffectLaunch } from '../../fixtures/quota-effect-launch.ts';
import { shortTemporaryRoot } from '../../fixtures/short-temporary-root.ts';
import type { QuotaEffectScenario } from './quota-fixture-contract.ts';
import {
	quotaFixtureLayout,
	retainQuotaFixture,
	stageQuotaFixture,
} from './quota-fixture-layout.ts';
import type { QuotaFixtureLayout } from './quota-fixture-layout.ts';

const roots: string[] = [];
afterEach(() => {
	for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
});
function setup(scenario: QuotaEffectScenario): {
	readonly layout: QuotaFixtureLayout;
	readonly evidence: string;
	readonly product: string;
} {
	const root = realpathSync(mkdtempSync(join(shortTemporaryRoot(), '2026-11-ql-')));
	roots.push(root);
	const evidence = join(root, 'evidence');
	mkdirSync(evidence, { mode: 0o700 });
	const product = join(root, 'libtest'),
		standin = join(root, 'quota-login-recording-standin');
	for (const path of [product, standin]) {
		writeFileSync(path, 'not executed');
		chmodSync(path, 0o700);
	}
	const layout = quotaFixtureLayout({
		scenario,
		product: captureArtifact(product),
		standin: captureArtifact(standin),
		repository: root,
		evidence,
	});
	if (existsSync(layout.output)) throw new Error('fixture output unexpectedly exists; preserve it');
	return { layout, evidence, product };
}
function stageOwned(layout: QuotaFixtureLayout, evidence: string): void {
	try {
		stageQuotaFixture(layout, evidence);
	} finally {
		if (existsSync(layout.output)) roots.push(layout.output);
	}
}
test.each(['captured-actions', 'login-succeeded', 'grouping-reopen'] as const)(
	'pure %s layout performs no IO; staging admits the same exact wrapper bytes',
	(scenario) => {
		const { layout, evidence } = setup(scenario);
		expect(existsSync(layout.output)).toBe(false);
		expect(Buffer.byteLength(layout.configuration.controlSocket)).toBeLessThanOrEqual(103);
		stageOwned(layout, evidence);
		expect(readFileSync(layout.commandPath, 'utf8')).toBe(layout.commandBytes);
		expect(readFileSync(layout.configurationPath, 'utf8')).toBe(layout.configurationBytes);
		expect(quotaEffectLaunch(layout.command).arguments).toEqual([
			'--exact',
			'quota::quota_account_adapter::tests::effectful_fixture::compiled_quota_effectful_fixture',
			'--ignored',
			'--nocapture',
			'--test-threads=1',
		]);
		for (const path of [
			layout.configuration.events,
			layout.configuration.receipt,
			layout.configuration.dispatchReceipt,
			layout.configuration.controlSocket,
		])
			expect(existsSync(path)).toBe(false);
		writeFileSync(
			layout.configuration.events,
			'actual producer bytes retained after an owned settlement',
		);
		retainQuotaFixture(layout, evidence);
		expect(readFileSync(join(evidence, 'quota-fixture', 'events.jsonl'), 'utf8')).toBe(
			'actual producer bytes retained after an owned settlement',
		);
	},
);
test('a replaced product is rejected before creating the planned fixture output', () => {
	const { layout, evidence, product } = setup('captured-actions');
	writeFileSync(product, 'changed after admission');
	expect(() => stageOwned(layout, evidence)).toThrow('artifact changed');
	expect(existsSync(layout.output)).toBe(false);
});
test('staging never reuses an earlier invocation directory or overwrites its files', () => {
	const { layout, evidence } = setup('captured-actions');
	stageOwned(layout, evidence);
	expect(() => stageQuotaFixture(layout, evidence)).toThrow();
	expect(readFileSync(layout.commandPath, 'utf8')).toBe(layout.commandBytes);
});
