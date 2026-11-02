/** Admit every declared quota phase before staging; held removal runs in its own real child. */
import { mkdirSync, readFileSync, realpathSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { processIdentitySchema } from '../../evidence/evidence-schema.ts';
import type { ProcessIdentity } from '../../evidence/evidence-schema.ts';
import {
	lookupProcessIdentity,
	requireOwnedProductChild,
	sameIdentity,
} from '../../processes/process-identity.ts';
import type { AuthoredFlowPlan } from './authored-flow-plan.ts';
import type { FlowExecutionResult, FlowInvocation } from './flow-executor.ts';
import { admitFlow } from './flow-executor.ts';
import { nativeFlowCapabilities } from './native-flow-runtime.ts';
import { quotaJourneyScenarios } from './quota-fixture-contract.ts';
import { quotaFlowBundle } from './quota-flow-bindings.ts';
import type { QuotaFlowBundle } from './quota-flow-bindings.ts';
import { quotaExpectedKey, quotaScreen } from './quota-flow-inputs.ts';
import { executeQuotaFlow } from './quota-flow-run.ts';

export interface QuotaCasePhase {
	readonly invocation: FlowInvocation;
	readonly bundle: QuotaFlowBundle;
	readonly evidence: string;
}
export function quotaCasePhases(
	plan: AuthoredFlowPlan,
	invocation: FlowInvocation,
	output: string,
): readonly QuotaCasePhase[] | null {
	const scenarios = quotaJourneyScenarios(invocation.journey.journeyId);
	const split = invocation.journey.steps.findIndex(
		(step) => step.kind === 'fixture' && step.operation === 'repeat-through-menu-with-details-open',
	);
	const definitions =
		split < 0
			? scenarios.map((scenario) => ({ name: scenario, scenario, invocation }))
			: scenarios.map((scenario, index) => ({
					name: index === 0 ? 'key' : 'menu',
					scenario,
					invocation: {
						...invocation,
						journey: {
							...invocation.journey,
							steps:
								index === 0
									? invocation.journey.steps.slice(0, split)
									: invocation.journey.steps.slice(split),
						},
					},
				}));
	if (definitions.length === 0) return null;
	const first = definitions[0];
	if (
		first === undefined ||
		quotaFlowBundle(plan, first.invocation, output, first.scenario) === null
	)
		return null;
	if (definitions.length === 1) {
		const bundle = quotaFlowBundle(plan, first.invocation, output, first.scenario);
		if (bundle === null) throw new Error('quota adapter absent');
		return [{ invocation: first.invocation, bundle, evidence: output }];
	}
	return definitions.map((definition) => {
		const evidence = join(output, definition.name),
			bundle = quotaFlowBundle(plan, definition.invocation, evidence, definition.scenario);
		if (bundle === null) throw new Error('quota phase adapter absent');
		const operation = 'retain-quota-phase-processes';
		const phaseInvocation: FlowInvocation = {
			...invocation,
			journey: {
				...invocation.journey,
				steps: [
					{
						kind: 'fixture',
						operation,
						purpose:
							'Retain actual driver, wrapper and direct product identities before the original phase steps.',
					},
					...definition.invocation.journey.steps,
				],
			},
		};
		const fixtures = new Map(bundle.bindings.fixtures);
		fixtures.set(operation, async (context) => {
			const driver = lookupProcessIdentity(context.driver.processId());
			if (
				driver === null ||
				driver.realUid !== process.getuid?.() ||
				realpathSync(driver.executable) !== plan.driver.path
			)
				throw new Error('quota phase driver identity differs');
			const wrapper = requireOwnedProductChild(driver.pid, plan.node.path),
				product = requireOwnedProductChild(wrapper.pid, plan.product.path);
			writeFileSync(
				join(evidence, 'phase-processes.json'),
				JSON.stringify({ driver, wrapper, product }, null, 2),
			);
			return context.input.press('F12', [quotaExpectedKey('F12', quotaScreen)]);
		});
		return {
			invocation: phaseInvocation,
			evidence,
			bundle: {
				...bundle,
				bindings: {
					...bundle.bindings,
					fixtures,
					inputs: new Map(
						[...bundle.bindings.inputs].map(([index, binding]) => [index + 1, binding]),
					),
				},
			},
		};
	});
}
export async function executeQuotaCase(
	plan: AuthoredFlowPlan,
	invocation: FlowInvocation,
	output: string,
	verifyPins: () => void,
): Promise<FlowExecutionResult> {
	const phases = quotaCasePhases(plan, invocation, output);
	if (phases === null) throw new Error('quota case adapter absent');
	for (const phase of phases) {
		const gaps = admitFlow(phase.invocation, nativeFlowCapabilities(phase.bundle.bindings));
		if (gaps.length > 0) throw new Error('quota phase admission gap: ' + JSON.stringify(gaps));
	}
	const observations: { scenario: string; output: string; result: unknown }[] = [],
		completed: Extract<FlowExecutionResult, { kind: 'completed' }>[] = [],
		identities: ProcessIdentity[] = [];
	let completedAll = false;
	try {
		await phases.reduce(async (previous, phase) => {
			await previous;
			verifyPins();
			if (phase.evidence !== output) mkdirSync(phase.evidence, { mode: 0o700 });
			const scenario = phase.bundle.layout.configuration.scenario;
			try {
				const result = await executeQuotaFlow(phase.invocation, phase.bundle, phase.evidence);
				if (result.kind !== 'completed') throw new Error('quota phase was not admitted');
				if (phases.length > 1) {
					const record: unknown = JSON.parse(
						readFileSync(join(phase.evidence, 'phase-processes.json'), 'utf8'),
					);
					const source = processIdentitySchema.parse(
						record !== null && typeof record === 'object' && 'driver' in record
							? record.driver
							: null,
					);
					if (identities.some((prior) => sameIdentity(source, prior)))
						throw new Error('quota phases reused a PTY driver');
					identities.push(source);
				}
				completed.push(result);
				observations.push({ scenario, output: phase.evidence, result });
				verifyPins();
			} catch (error) {
				observations.push({
					scenario,
					output: phase.evidence,
					result: {
						kind: 'failed',
						message: error instanceof Error ? error.message : String(error),
					},
				});
				throw error;
			}
		}, Promise.resolve());
		const finish = completed.at(-1)?.finish;
		if (finish === undefined) throw new Error('quota case has no completed phases');
		completedAll = true;
		return { kind: 'completed', steps: completed.flatMap((result) => result.steps), finish };
	} finally {
		writeFileSync(
			join(output, 'quota-case-phases.json'),
			JSON.stringify(
				{
					planned: phases.map((phase) => phase.bundle.layout.configuration.scenario),
					completedAll,
					observations,
					driverIdentities: identities,
				},
				null,
				2,
			),
		);
	}
}
