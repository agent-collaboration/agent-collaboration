import { existsSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { afterEach, expect, test } from 'vitest';

import type { ArtifactBinding } from '../../evidence/evidence-schema.ts';
import { captureArtifact, sourceDigest } from '../../evidence/source-binding.ts';
import { protocolPlan, temporaryRoot } from '../../fixtures/receipt-fixtures.ts';
import { authoredFlowPlanSchema } from './authored-flow-plan.ts';
import type { AuthoredFlowPlan } from './authored-flow-plan.ts';
import { authoredFlowMatrix, requireAuthoredFlowAdmission } from './authored-flow-run.ts';
import { quotaCasePhases } from './quota-case-run.ts';
import { quotaFlowBundle } from './quota-flow-bindings.ts';

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
test('a valid bound artifact does not admit an unauthored quota journey', () => {
	const { plan, standin } = controlledPlan('quota-outside-authored-catalogue');
	expect(() => requireAuthoredFlowAdmission({ ...plan, quota: { loginStandin: standin } })).toThrow(
		'unknown authored flow',
	);
	expect(existsSync(plan.output)).toBe(false);
});
test('non-guard quota refresh admits both phases without requiring a stand-in', () => {
	const { plan } = controlledPlan('quota-refresh-partial');
	expect(() => requireAuthoredFlowAdmission(plan)).not.toThrow();
	expect(existsSync(plan.output)).toBe(false);
});

test.each([
	'quota-login-succeeded',
	'quota-login-cancelled-130',
	'quota-login-failed',
	'quota-group-by-f6',
])(
	'%s admits its complete native binding without creating output or connecting a socket',
	(journey) => {
		const { plan, standin } = controlledPlan(journey);
		expect(() =>
			requireAuthoredFlowAdmission({ ...plan, quota: { loginStandin: standin } }),
		).not.toThrow();
		expect(existsSync(plan.output)).toBe(false);
	},
);

test.each([
	'quota-actions-every-command',
	'quota-remove-cancel-primary',
	'quota-remove-middle',
	'quota-remove-last',
	'quota-remove-only',
	'quota-remove-requires-disabled-refusal',
])('%s admits all declared account action steps before output creation', (journey) => {
	const { plan } = controlledPlan(journey);
	expect(() => requireAuthoredFlowAdmission(plan)).not.toThrow();
	expect(existsSync(plan.output)).toBe(false);
});

test('quota toolbar and menu admits its complete read-only adapter before output creation', () => {
	const { plan } = controlledPlan('quota-toolbar-and-menu');
	expect(() => requireAuthoredFlowAdmission(plan)).not.toThrow();
	expect(existsSync(plan.output)).toBe(false);
});

test('middle removal binds its separate declared held phase before output creation', () => {
	const { plan } = controlledPlan('quota-remove-middle'),
		invocation = authoredFlowMatrix(plan)[0];
	if (invocation === undefined) throw new Error('literal invocation absent');
	const held = quotaFlowBundle(plan, invocation, plan.output, 'remove-held');
	expect(held?.layout.configuration.scenario).toBe('remove-held');
	expect(existsSync(plan.output)).toBe(false);
});
test('a quota phase rejects a scenario outside its authored journey before output creation', () => {
	const { plan } = controlledPlan('quota-remove-middle'),
		invocation = authoredFlowMatrix(plan)[0];
	if (invocation === undefined) throw new Error('literal invocation absent');
	expect(() => quotaFlowBundle(plan, invocation, plan.output, 'login-failed7')).toThrow(
		'outside authored journey',
	);
	expect(existsSync(plan.output)).toBe(false);
});

test('middle removal admits both fresh phases with every original step and no output', () => {
	const { plan } = controlledPlan('quota-remove-middle'),
		invocation = authoredFlowMatrix(plan)[0];
	if (invocation === undefined) throw new Error('literal invocation absent');
	const phases = quotaCasePhases(plan, invocation, plan.output);
	expect(phases?.map((phase) => phase.bundle.layout.configuration.scenario)).toEqual([
		'remove-middle',
		'remove-held',
	]);
	expect(new Set(phases?.map((phase) => phase.bundle.layout.output)).size).toBe(2);
	for (const phase of phases ?? [])
		expect(phase.invocation.journey.steps.slice(1)).toEqual(invocation.journey.steps);
	expect(existsSync(plan.output)).toBe(false);
});

test.each(['quota-list-option-navigation', 'quota-help-option-navigation'])(
	'%s admits the full read-only physical binding without output',
	(journey) => {
		const { plan } = controlledPlan(journey);
		expect(() => requireAuthoredFlowAdmission(plan)).not.toThrow();
		expect(existsSync(plan.output)).toBe(false);
	},
);

test.each([
	'quota-refresh-all-success',
	'quota-refresh-partial',
	'quota-refresh-all-failed',
	'quota-refresh-setup-failed',
])('%s pre-admits separate key and menu children', (journey) => {
	const { plan } = controlledPlan(journey),
		invocation = authoredFlowMatrix(plan)[0];
	if (invocation === undefined) throw new Error('literal invocation absent');
	expect(() => requireAuthoredFlowAdmission(plan)).not.toThrow();
	const phases = quotaCasePhases(plan, invocation, plan.output);
	expect(phases).toHaveLength(2);
	expect(new Set(phases?.map((phase) => phase.bundle.layout.output)).size).toBe(2);
	expect(
		phases
			?.flatMap((phase) => phase.invocation.journey.steps.slice(1))
			.filter((step) => step.kind === 'readback')
			.map((step) => step.checks),
	).toEqual(
		invocation.journey.steps.filter((step) => step.kind === 'readback').map((step) => step.checks),
	);
	expect(existsSync(plan.output)).toBe(false);
});
