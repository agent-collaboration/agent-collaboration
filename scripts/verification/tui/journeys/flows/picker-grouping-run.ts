/** One R19 case spans real fresh picker processes; no fixture snapshot substitutes for reopen. */
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { z } from 'zod';

import { processIdentitySchema } from '../../evidence/evidence-schema.ts';
import { fileDigest } from '../../evidence/source-binding.ts';
import { sameIdentity } from '../../processes/process-identity.ts';
import type { AuthoredFlowPlan } from './authored-flow-plan.ts';
import type { FlowJourney } from './flow-definition.ts';
import type { FlowExecutionResult, FlowInvocation } from './flow-executor.ts';
import { executeNativeFlow } from './native-flow-runtime.ts';
import { pickerGroupingBindings } from './picker-grouping-bindings.ts';
import { pickerGroupingFallbackJourney, pickerGroupingReopenJourney } from './picker-grouping.ts';
import {
	createUnreadablePickerPreferences,
	readPickerRememberedGrouping,
	seedUnknownPickerGrouping,
} from './picker-preferences-fixture.ts';

const observationSchema = z.object({ driverIdentity: processIdentitySchema });

export async function executePickerGroupingCase(
	plan: AuthoredFlowPlan,
	invocation: FlowInvocation,
	output: string,
	verifyPins: () => void,
): Promise<FlowExecutionResult> {
	const preferences = join(output, 'preferences');
	mkdirSync(preferences, { mode: 0o700 });
	const phases: { name: string; output: string; result: unknown }[] = [];
	const completed: Extract<FlowExecutionResult, { kind: 'completed' }>[] = [];
	const identities: z.infer<typeof processIdentitySchema>[] = [];
	let completedAll = false;
	const runPhase = async (name: string, journey: FlowJourney, directory: string): Promise<void> => {
		verifyPins();
		const evidence = join(output, name),
			receipts = join(evidence, 'receipts');
		mkdirSync(evidence, { mode: 0o700 });
		mkdirSync(receipts, { mode: 0o700 });
		const phaseInvocation = { ...invocation, journey };
		const bindings = pickerGroupingBindings(plan, phaseInvocation, evidence, receipts, directory);
		if (bindings === null) throw new Error('grouping phase adapter absent');
		try {
			const result = await executeNativeFlow(phaseInvocation, bindings);
			phases.push({ name, output: evidence, result });
			if (result.kind !== 'completed') throw new Error('grouping phase not admitted: ' + name);
			const firstFacet = journey === pickerGroupingReopenJourney ? 'provider' : 'none';
			const observation = observationSchema.parse(
				JSON.parse(readFileSync(join(evidence, 'grouping-1-' + firstFacet + '.json'), 'utf8')),
			);
			if (identities.some((previous) => sameIdentity(observation.driverIdentity, previous)))
				throw new Error('grouping reopen reused an earlier PTY driver process');
			identities.push(observation.driverIdentity);
			completed.push(result);
			verifyPins();
		} catch (error) {
			if (!phases.some((phase) => phase.name === name))
				phases.push({
					name,
					output: evidence,
					result: {
						kind: 'failed',
						message: error instanceof Error ? error.message : String(error),
					},
				});
			throw error;
		}
	};
	try {
		await runPhase('initial', invocation.journey, preferences);
		if (readPickerRememberedGrouping(plan.observerRepository, preferences) !== 'provider')
			throw new Error('first real picker did not drain its remembered Provider preference');
		await runPhase('reopened', pickerGroupingReopenJourney, preferences);
		if (readPickerRememberedGrouping(plan.observerRepository, preferences) !== 'provider')
			throw new Error('reopened picker changed the saved Provider preference without a choice');
		const unknown = seedUnknownPickerGrouping(plan.observerRepository, preferences);
		writeFileSync(join(output, 'unknown-preference-seed.json'), JSON.stringify(unknown, null, 2));
		await runPhase('unknown-value', pickerGroupingFallbackJourney, preferences);
		if (readPickerRememberedGrouping(plan.observerRepository, preferences) !== unknown.after)
			throw new Error('unknown remembered grouping was opportunistically rewritten');
		const unreadable = join(output, 'unreadable-preferences');
		mkdirSync(unreadable, { mode: 0o700 });
		const damaged = createUnreadablePickerPreferences(plan.observerRepository, unreadable);
		writeFileSync(
			join(output, 'unreadable-preference-seed.json'),
			JSON.stringify(damaged, null, 2),
		);
		await runPhase('unreadable-value', pickerGroupingFallbackJourney, unreadable);
		if (fileDigest(damaged.path) !== damaged.sha256)
			throw new Error('unreadable preference evidence was replaced');
		const finish = completed.at(-1)?.finish;
		if (finish === undefined) throw new Error('grouping phases did not finish');
		completedAll = true;
		return { kind: 'completed', steps: completed.flatMap((result) => result.steps), finish };
	} finally {
		writeFileSync(
			join(output, 'grouping-persistence-phases.json'),
			JSON.stringify(
				{
					status: completedAll ? 'completed' : 'incomplete',
					phases,
					driverIdentities: identities,
					preferencesDirectory: preferences,
					physicalTerminal: false,
					finalAcceptance: false,
				},
				null,
				2,
			),
		);
	}
}
