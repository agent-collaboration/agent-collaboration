import { existsSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { afterEach, expect, test } from 'vitest';

import type { ArtifactBinding } from '../../evidence/evidence-schema.ts';
import { captureArtifact, sourceDigest } from '../../evidence/source-binding.ts';
import { protocolPlan, temporaryRoot } from '../../fixtures/receipt-fixtures.ts';
import { authoredFlowPlanSchema } from './authored-flow-plan.ts';
import type { AuthoredFlowPlan } from './authored-flow-plan.ts';
import { requireAuthoredFlowAdmission } from './authored-flow-run.ts';

const roots: string[] = [];
afterEach(() => {
	for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
});
function controlledPlan(journey: string): {
	readonly root: string;
	readonly standin: ArtifactBinding;
	readonly plan: AuthoredFlowPlan;
} {
	const root = temporaryRoot();
	roots.push(root);
	const fixture = protocolPlan(root),
		path = join(root, 'source-bound-standin');
	writeFileSync(path, 'controlled stand-in artifact, never executed');
	const source = {
		productSource: fixture.source,
		productSourceDigest: sourceDigest(fixture.source),
		observerSource: fixture.source,
		observerSourceDigest: sourceDigest(fixture.source),
	};
	return {
		root,
		standin: captureArtifact(path),
		plan: authoredFlowPlanSchema.parse({
			...source,
			productRepository: root,
			observerRepository: root,
			driver: fixture.capture_driver,
			product: fixture.harness_binary,
			node: fixture.node,
			chrome: '/unused/no-launch',
			output: join(root, 'not-created'),
			journey,
			grids: ['120x36'],
			keys: ['legacy'],
		}),
	};
}
test('a quota plan explicitly binds the stand-in artifact and rejects an invalid digest shape', () => {
	const { plan, standin } = controlledPlan('quota-login-succeeded');
	expect(() =>
		authoredFlowPlanSchema.parse({ ...plan, quota: { loginStandin: standin } }),
	).not.toThrow();
	expect(() =>
		authoredFlowPlanSchema.parse({
			...plan,
			quota: { loginStandin: { ...standin, sha256: 'not-a-digest' } },
		}),
	).toThrow();
});
test.each([
	'quota-login-succeeded',
	'quota-login-cancelled-130',
	'quota-login-failed',
	'quota-group-by-f6',
])('%s requires a source-bound stand-in before adapter lookup or output creation', (journey) => {
	const { plan } = controlledPlan(journey);
	expect(() => requireAuthoredFlowAdmission(plan)).toThrow(
		'requires a source-bound login stand-in',
	);
	expect(existsSync(plan.output)).toBe(false);
});
test('a wrong stand-in hash rejects before any native adapter or output is constructed', () => {
	const { plan, standin } = controlledPlan('quota-login-succeeded');
	const candidate = { ...plan, quota: { loginStandin: { ...standin, sha256: '0'.repeat(64) } } };
	expect(() => requireAuthoredFlowAdmission(candidate)).toThrow('artifact changed');
	expect(existsSync(plan.output)).toBe(false);
});
test('replacement of a previously bound stand-in remains an admission failure', () => {
	const { plan, standin } = controlledPlan('quota-login-succeeded');
	writeFileSync(standin.path, 'replaced stand-in');
	expect(() => requireAuthoredFlowAdmission({ ...plan, quota: { loginStandin: standin } })).toThrow(
		'artifact changed',
	);
	expect(existsSync(plan.output)).toBe(false);
});
test('a valid bound artifact does not turn an unsupported quota adapter into a pass', () => {
	const { plan, standin } = controlledPlan('quota-login-succeeded');
	expect(() => requireAuthoredFlowAdmission({ ...plan, quota: { loginStandin: standin } })).toThrow(
		'no native adapter',
	);
	expect(existsSync(plan.output)).toBe(false);
});
test('non-guard quota routes preserve their unsupported-adapter gap without requiring a stand-in', () => {
	const { plan } = controlledPlan('quota-refresh-partial');
	expect(() => requireAuthoredFlowAdmission(plan)).toThrow('no native adapter');
	expect(existsSync(plan.output)).toBe(false);
});
