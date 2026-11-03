/** Real direct-picker startup qualification, requiring an explicit bounded native grant.
 * Starts the declared product; no wrapper/no-product handshake is claimed.
 */
import { spawnSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import {
	isMain,
	planArgument,
	reportCliFailure,
	strictArguments,
} from '../../cli/cli-arguments.ts';
import { lookupProcessIdentity, sameIdentity } from '../../processes/process-identity.ts';
import { readSourceQualifiedOutcome } from '../../sessions/source-qualified-outcome.ts';
import { MarkerInputSession } from '../../terminal/marker-input-session.ts';
import { PtyGridDriver } from '../../terminal/pty-grid-driver.ts';
import { renderOriginal } from '../../terminal/terminal-original.ts';
import { authoredFlowPlanSchema } from './authored-flow-plan.ts';
import type { AuthoredFlowPlan } from './authored-flow-plan.ts';
import {
	authoredFlowMatrix,
	checkAuthoredFlowTarget,
	requireAuthoredFlowAdmission,
} from './authored-flow-run.ts';
import { pickerFlowBindings } from './picker-flow-bindings.ts';

export async function qualifyPickerFlowTransport(plan: AuthoredFlowPlan): Promise<void> {
	requireAuthoredFlowAdmission(plan);
	await checkAuthoredFlowTarget(plan);
	const output = plan.output + '-startup-qualification';
	mkdirSync(output, { mode: 0o700 });
	const observations: unknown[] = [];
	await plan.keys.reduce<Promise<void>>(async (previous, keys) => {
		await previous;
		const invocation = authoredFlowMatrix(plan).find((caseValue) => caseValue.keys === keys);
		if (invocation === undefined) throw new Error('qualified mode absent from authored matrix');
		const evidence = join(output, keys),
			receipts = join(evidence, 'receipts');
		mkdirSync(evidence, { mode: 0o700 });
		mkdirSync(receipts, { mode: 0o700 });
		const binding = pickerFlowBindings(plan, invocation, evidence, receipts);
		if (binding === null) throw new Error('picker binding disappeared');
		let driver: PtyGridDriver | null = null;
		try {
			driver = await PtyGridDriver.start(binding.target);
			const driverIdentity = lookupProcessIdentity(driver.processId());
			if (driverIdentity === null) throw new Error('live driver identity absent');
			await driver.waitForGrid('declared direct-picker startup', binding.ready);
			if (driver.inputModes().kittyFlags !== binding.expectedKeyboardFlags)
				throw new Error('startup keyboard flags differ from source');
			const inventory = spawnSync('/bin/ps', ['-axo', 'pid=,ppid=,comm='], { encoding: 'utf8' });
			if (inventory.status !== 0) throw new Error('startup child inventory unavailable');
			const childIds = inventory.stdout.split('\n').flatMap((line) => {
				const match = /^(\d+)\s+(\d+)\s+(.+)$/u.exec(line.trim());
				return match !== null &&
					Number(match[2]) === driverIdentity.pid &&
					match[3] === plan.product.path
					? [Number(match[1])]
					: [];
			});
			if (childIds.length !== 1)
				throw new Error('direct Rust owner must have exactly one declared product child');
			const childId = childIds[0];
			if (childId === undefined) throw new Error('direct product child PID absent');
			const productIdentity = lookupProcessIdentity(childId);
			if (productIdentity === null) throw new Error('live product identity absent');
			writeFileSync(
				join(evidence, 'owned-identities.json'),
				JSON.stringify({ driverIdentity, productIdentity }, null, 2),
			);
			const input = new MarkerInputSession({
				driver,
				receipt: binding.receipt,
				profile: 'terminal-only',
			});
			const marked = await input.press(binding.initialBarrier.key, binding.initialBarrier.expected);
			const frame = marked.frames.at(-1);
			if (frame === undefined || !binding.ready(frame.grid))
				throw new Error('initial scene lacks its own exact marker-bound frame');
			await renderOriginal({
				...binding.original,
				output: evidence,
				checkpoint: 'startup',
				bytes: frame.bytes,
				grid: frame.grid,
				resizes: frame.resizes,
				columns: 120,
				rows: 36,
			});
			const exited = await input.exit({
				expected: binding.quit.expected,
				inject: async () => {
					const delivered = await input.input.key(binding.quit.key);
					if (delivered.delivery.kind !== 'sent') throw new Error('startup quit was not delivered');
				},
			});
			readSourceQualifiedOutcome(join(receipts, 'picker-outcome.json'), { kind: 'cancelled' });
			const driverExit = await driver.stop();
			if (
				driverExit !== 0 ||
				sameIdentity(lookupProcessIdentity(driverIdentity.pid), driverIdentity) ||
				sameIdentity(lookupProcessIdentity(productIdentity.pid), productIdentity)
			)
				throw new Error('startup-owned processes did not settle');
			observations.push({
				keys,
				driverExit,
				productExit: exited.exitCode,
				driverIdentity,
				productIdentity,
				initialMarker: frame.binding,
				wrapper: 'not used by this direct picker route',
				productPasses: 0,
				evidence,
			});
		} finally {
			await driver?.stop();
		}
	}, Promise.resolve());
	await checkAuthoredFlowTarget(plan);
	writeFileSync(
		join(output, 'transport-receipt.json'),
		JSON.stringify(
			{
				kind: 'direct-picker-startup',
				productSource: plan.productSource.head,
				observerSource: plan.observerSource.head,
				product: plan.product,
				driver: plan.driver,
				observations,
				productPasses: 0,
			},
			null,
			2,
		),
	);
	process.stdout.write(JSON.stringify({ output, observations, productPasses: 0 }, null, 2) + '\n');
}
if (isMain(import.meta.url)) {
	try {
		const { values, positionals } = strictArguments(
			process.argv.slice(2),
			{ help: { type: 'boolean' } },
			true,
		);
		if (values.help)
			process.stdout.write(
				'qualify-picker-flow-transport.ts PLAN.json: requires a bounded native grant; launches the signed direct picker, records initial F12 ACK/frame/PNG, quits and verifies cleanup. It runs no B flow and is not a no-product wrapper handshake.\n',
			);
		else
			await qualifyPickerFlowTransport(
				authoredFlowPlanSchema.parse(JSON.parse(readFileSync(planArgument(positionals), 'utf8'))),
			);
	} catch (error) {
		reportCliFailure(error);
	}
}
