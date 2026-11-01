/** Execute one authored journey over a declared matrix; unsupported adapters never launch.
 * node scripts/verification/tui/journeys/flows/authored-flow-run.ts PLAN.json [--check]
 */
import { execFileSync } from 'node:child_process';
import { mkdirSync, readFileSync, realpathSync, writeFileSync } from 'node:fs';
import { isAbsolute, join } from 'node:path';

import {
	isMain,
	planArgument,
	reportCliFailure,
	strictArguments,
} from '../../cli/cli-arguments.ts';
import {
	canonicalJson,
	sourceBinding,
	sourceDigest,
	verifyArtifact,
} from '../../evidence/source-binding.ts';
import { ptyGridDriverEntry } from '../../terminal/pty-grid-driver.ts';
import { qualifyTerminal } from '../../terminal/terminal-qualification.ts';
import { authoredFlowPlanSchema } from './authored-flow-plan.ts';
import type { AuthoredFlowPlan } from './authored-flow-plan.ts';
import { authoredFlows, validateAuthoredFlows } from './flow-catalogue.ts';
import { flowObservationRule } from './flow-definition.ts';
import type { FlowInvocation, FlowExecutionResult } from './flow-executor.ts';
import { executeNativeFlow } from './native-flow-runtime.ts';
import { pickerFlowAdmission } from './picker-flow-admission.ts';
import { pickerFlowBindings } from './picker-flow-bindings.ts';
import { executePickerGroupingCase } from './picker-grouping-run.ts';
import { requireQuotaPlanArtifacts } from './quota-plan-admission.ts';

export function authoredFlowMatrix(plan: AuthoredFlowPlan): readonly FlowInvocation[] {
	validateAuthoredFlows();
	const journey = authoredFlows.find((flow) => flow.journeyId === plan.journey);
	if (journey === undefined) throw new Error('unknown authored flow ' + plan.journey);
	if (plan.keys.some((keys) => !journey.keys.includes(keys)))
		throw new Error('declared protocol is outside the authored journey: ' + plan.journey);
	return plan.grids.flatMap((grid) => plan.keys.map((keys) => ({ journey, grid, keys })));
}
export function requireAuthoredFlowAdmission(plan: AuthoredFlowPlan): void {
	requireQuotaPlanArtifacts(plan);
	for (const invocation of authoredFlowMatrix(plan)) {
		const bindings = pickerFlowBindings(plan, invocation, plan.output, plan.output);
		const gaps = pickerFlowAdmission(invocation, bindings);
		if (gaps.length > 0) throw new Error('fixture admission gap: ' + JSON.stringify(gaps));
	}
}
function requireSourcePins(plan: AuthoredFlowPlan): void {
	for (const [repository, source, digest] of [
		[plan.productRepository, plan.productSource, plan.productSourceDigest],
		[plan.observerRepository, plan.observerSource, plan.observerSourceDigest],
	] as const)
		if (
			canonicalJson(sourceBinding(repository)) !== canonicalJson(source) ||
			sourceDigest(source) !== digest
		)
			throw new Error('authored-flow declared source changed: ' + repository);
	for (const artifact of [plan.driver, plan.product, plan.node]) verifyArtifact(artifact);
}
export async function checkAuthoredFlowTarget(plan: AuthoredFlowPlan): Promise<void> {
	if (
		![plan.productRepository, plan.observerRepository, plan.output, plan.chrome].every(isAbsolute)
	)
		throw new Error('authored-flow paths must be absolute');
	if (process.version !== 'v26.12.1' || realpathSync(process.execPath) !== plan.node.path)
		throw new Error('declared pinned Node required');
	requireSourcePins(plan);
	const preflight = plan.expectationPreflight;
	if (preflight === undefined)
		throw new Error('source-derived receipt expectation preflight required');
	const expectedCases = authoredFlowMatrix(plan)
		.map((invocation) => invocation.grid + '/' + invocation.keys)
		.toSorted();
	const actualCases = preflight.cases.map((entry) => entry.grid + '/' + entry.keys).toSorted();
	if (canonicalJson(expectedCases) !== canonicalJson(actualCases))
		throw new Error('expectation preflight does not cover the exact declared matrix');
	for (const entry of preflight.cases)
		if (entry.inputs.some((input) => input.minimumRepeats > input.maximumRepeats))
			throw new Error('invalid expectation repetition range');
	for (const source of preflight.sources) verifyArtifact(source);
	for (const retained of preflight.retainedComparisons) verifyArtifact(retained);
	for (const artifact of [plan.driver, plan.product])
		execFileSync('/usr/bin/codesign', ['--verify', '--strict', artifact.path]);
	const invocation = authoredFlowMatrix(plan)[0];
	if (invocation === undefined) throw new Error('authored-flow matrix is empty');
	const entry = ptyGridDriverEntry(invocation.journey.surface);
	const listing = execFileSync(plan.driver.path, [entry, '--exact', '--list'], {
		encoding: 'utf8',
	});
	if (!listing.split('\n').includes(entry + ': test'))
		throw new Error('streaming driver entry absent');
	await qualifyTerminal();
}
export async function runAuthoredFlow(plan: AuthoredFlowPlan): Promise<void> {
	const matrix = authoredFlowMatrix(plan);
	// Probe the adapter registry before any mkdir or child construction.
	requireAuthoredFlowAdmission(plan);
	await checkAuthoredFlowTarget(plan);
	mkdirSync(plan.output, { mode: 0o700 });
	writeFileSync(join(plan.output, 'declared-plan.json'), JSON.stringify(plan, null, 2));
	const results: { grid: string; keys: string; result: unknown }[] = [];
	try {
		await matrix.reduce(async (previous, invocation) => {
			await previous;
			const output = join(plan.output, invocation.keys + '-' + invocation.grid);
			mkdirSync(output, { mode: 0o700 });
			let result: FlowExecutionResult;
			try {
				if (plan.journey === 'sessions-group-by-f6') {
					result = await executePickerGroupingCase(plan, invocation, output, () =>
						requireSourcePins(plan),
					);
				} else {
					const receipts = join(output, 'receipts');
					mkdirSync(receipts, { mode: 0o700 });
					const bindings = pickerFlowBindings(plan, invocation, output, receipts);
					if (bindings === null) throw new Error('fixture adapter disappeared');
					result = await executeNativeFlow(invocation, bindings);
				}
			} catch (error) {
				results.push({
					grid: invocation.grid,
					keys: invocation.keys,
					result: {
						kind: 'failed',
						message: error instanceof Error ? error.message : String(error),
					},
				});
				throw error;
			}
			results.push({ grid: invocation.grid, keys: invocation.keys, result });
			if (result.kind !== 'completed')
				throw new Error('native flow admission gaps: ' + JSON.stringify(result.gaps));
			requireSourcePins(plan);
		}, Promise.resolve());
	} finally {
		writeFileSync(
			join(plan.output, 'matrix-results.json'),
			JSON.stringify(
				{
					journey: plan.journey,
					planned: matrix.length,
					results,
					physicalTerminal: false,
					finalS8Acceptance: false,
					observationRule: flowObservationRule,
				},
				null,
				2,
			),
		);
	}
}
export async function authoredFlowMain(args: readonly string[]): Promise<void> {
	const { values, positionals } = strictArguments(
		args,
		{ help: { type: 'boolean' }, check: { type: 'boolean' } },
		true,
	);
	if (values.help) {
		process.stdout.write(
			'authored-flow-run.ts PLAN.json [--check]: one authored flow, exact split source/artifact pins, four-grid/key matrix, real ACK/frame/original-image evidence. --check launches no product. Unbound adapters fail admission.\n',
		);
		return;
	}
	const plan = authoredFlowPlanSchema.parse(
		JSON.parse(readFileSync(planArgument(positionals), 'utf8')),
	);
	const matrix = authoredFlowMatrix(plan);
	requireAuthoredFlowAdmission(plan);
	if (values.check) {
		await checkAuthoredFlowTarget(plan);
		process.stdout.write(
			JSON.stringify({ kind: 'qualified-not-driven', cases: matrix.length, productPasses: 0 }) +
				'\n',
		);
	} else await runAuthoredFlow(plan);
}
if (isMain(import.meta.url)) await authoredFlowMain(process.argv.slice(2)).catch(reportCliFailure);
