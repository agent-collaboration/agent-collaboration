/** One admitted quota invocation owns staging, native execution, final effects and raw-file retention. */
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';

import type { FlowExecutionResult, FlowInvocation } from './flow-executor.ts';
import { executeNativeFlow } from './native-flow-runtime.ts';
import { retainQuotaFixture, stageQuotaFixture } from './quota-fixture-layout.ts';
import type { QuotaFlowBundle } from './quota-flow-bindings.ts';

export async function executeQuotaFlow(
	invocation: FlowInvocation,
	bundle: QuotaFlowBundle,
	evidence: string,
): Promise<FlowExecutionResult> {
	stageQuotaFixture(bundle.layout, evidence);
	let settlement:
		| { readonly kind: 'completed'; readonly result: FlowExecutionResult }
		| { readonly kind: 'failed'; readonly error: unknown };
	try {
		const result = await executeNativeFlow(invocation, bundle.bindings);
		if (result.kind !== 'completed') throw new Error('quota binding was not fully admitted');
		const receipt = await bundle.fixture.verifyFinal();
		writeFileSync(join(evidence, 'quota-final-readback.json'), JSON.stringify(receipt, null, 2));
		settlement = { kind: 'completed', result };
	} catch (error) {
		settlement = { kind: 'failed', error };
	}
	const failures: unknown[] = [];
	try {
		await bundle.fixture.close();
	} catch (error) {
		failures.push(error);
	}
	try {
		retainQuotaFixture(bundle.layout, evidence);
	} catch (error) {
		failures.push(error);
	}
	if (settlement.kind === 'failed') {
		if (failures.length > 0)
			throw new AggregateError([settlement.error, ...failures], 'quota flow and retention failed');
		throw settlement.error;
	}
	if (failures.length > 0) throw new AggregateError(failures, 'quota owned retention failed');
	return settlement.result;
}
