import { spawnSync } from 'node:child_process';
/** Qualify the real TS/Rust/wrapper handshake without spawning the example. */
import { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { isMain, planArgument, reportCliFailure, strictArguments } from '../cli/cli-arguments.ts';
import { fileDigest, readJson } from '../evidence/source-binding.ts';
import { lookupProcessIdentity } from '../processes/process-identity.ts';
import { PtyGridDriver, ptyGridDriverArguments } from '../terminal/pty-grid-driver.ts';
import { gridLines } from '../terminal/terminal-grid.ts';
import { requireObservation, showcasePlanSchema } from './contracts.ts';
import type { ShowcasePlan } from './contracts.ts';
import { checkShowcaseTarget } from './run.ts';

export async function qualifyShowcaseTransport(plan: ShowcasePlan): Promise<void> {
	await checkShowcaseTarget(plan);
	const output = plan.output + '-transport-qualification';
	mkdirSync(output);
	const observations: unknown[] = [];
	await plan.keys.reduce<Promise<void>>(async (previous, keys) => {
		await previous;
		const evidence = join(output, keys);
		mkdirSync(evidence);
		const receipts = join(evidence, 'receipts');
		mkdirSync(receipts);
		const configuration = join(evidence, 'fixture-command.json');
		writeFileSync(
			configuration,
			JSON.stringify({
				surface: 'showcase',
				binary: plan.example.path,
				binary_sha256: plan.example.sha256,
				approved_cwd: plan.exampleSource.repository,
				output: evidence,
				temporary_directory: evidence,
				receipt_directory: receipts,
			}),
		);
		const admission = spawnSync(plan.driver.path, ptyGridDriverArguments('showcase'), {
			cwd: plan.repository,
			encoding: 'utf8',
			timeout: 30_000,
			env: {
				...process.env,
				TUI_GRID_CHECK_ONLY: '1',
				TUI_GRID_HANDSHAKE_ONLY: undefined,
				TUI_GRID_SURFACE: 'showcase',
				TUI_GRID_KEYS: keys,
				QUOTA_STORYBOARD_NODE: plan.node.path,
				QUOTA_STORYBOARD_NODE_SHA256: plan.node.sha256,
				TUI_SHOWCASE_OUTPUT: evidence,
				TUI_SHOWCASE_CONFIGURATION: configuration,
				TUI_SHOWCASE_CONFIGURATION_SHA256: fileDigest(configuration),
			},
		});
		writeFileSync(join(evidence, 'admission.stdout'), admission.stdout);
		writeFileSync(join(evidence, 'admission.stderr'), admission.stderr);
		requireObservation(
			admission.status === 0 &&
				admission.stdout.includes(`TUI_GRID CHECKED showcase ${keys} 120 36`),
			'exact driver arguments and launch binding are admitted before the composed handshake',
			{ exit: admission.status, error: admission.error?.message, stderr: admission.stderr },
		);
		requireObservation(
			!existsSync(join(evidence, 'quota-wrapper.pid')) &&
				!existsSync(join(evidence, 'quota-child.pid')),
			'argument admission launches no wrapper or example',
			evidence,
		);
		const driver = await PtyGridDriver.start({
			repository: plan.repository,
			driver: plan.driver.path,
			surface: 'showcase',
			keys,
			evidence,
			node: plan.node,
			handshakeOnly: true,
			showcaseEnvironment: {
				TUI_SHOWCASE_OUTPUT: evidence,
				TUI_SHOWCASE_CONFIGURATION: configuration,
				TUI_SHOWCASE_CONFIGURATION_SHA256: fileDigest(configuration),
			},
		});
		let finished = false;
		try {
			await driver.waitForGrid('admitted wrapper handshake', (grid) =>
				gridLines(grid).some((line) => line.includes('TUI_SHOWCASE_WRAPPER_HANDSHAKE_READY')),
			);
			const wrapperPid = Number(readFileSync(join(evidence, 'quota-wrapper.pid'), 'utf8'));
			requireObservation(
				lookupProcessIdentity(wrapperPid) !== null,
				'the real wrapper is alive at handshake',
				wrapperPid,
			);
			requireObservation(
				!existsSync(join(evidence, 'quota-child.pid')) && readdirSync(receipts).length === 0,
				'handshake stops before the example or any product receipt',
				evidence,
			);
			const exit = await driver.stop();
			finished = true;
			requireObservation(
				exit === 0 &&
					lookupProcessIdentity(wrapperPid) === null &&
					lookupProcessIdentity(driver.processId()) === null,
				'driver and wrapper settle without survivors',
				{ exit, wrapperPid },
			);
			requireObservation(
				readFileSync(join(evidence, 'wrapper-handshake.exit'), 'utf8') === '0' &&
					!existsSync(join(evidence, 'cleanup-unsettled')) &&
					!existsSync(join(evidence, 'quota-child.pid')) &&
					readdirSync(receipts).length === 0,
				'handshake creates only its own successful lifecycle receipt',
				evidence,
			);
			const stdout = readFileSync(join(evidence, 'driver.stdout'), 'utf8');
			requireObservation(
				stdout.includes('TUI_GRID READY 120 36 STREAM'),
				'the exact composed TS and Rust STREAM handshake was received',
				stdout,
			);
			observations.push({
				keys,
				admissionExit: admission.status,
				exit,
				wrapperPid,
				driverPid: driver.processId(),
				contract: 'READY 120 36 STREAM',
				exampleSpawned: false,
				evidence,
			});
		} finally {
			if (!finished) await driver.stop();
		}
	}, Promise.resolve());
	await checkShowcaseTarget(plan);
	const receipt = {
		kind: 'showcase-composed-transport',
		apparatus: plan.source,
		exampleSource: plan.exampleSource,
		driver: plan.driver,
		example: plan.example,
		observations,
	};
	writeFileSync(join(output, 'transport-receipt.json'), JSON.stringify(receipt, null, 2) + '\n');
	process.stdout.write(JSON.stringify({ output, observations }, null, 2) + '\n');
}
if (isMain(import.meta.url))
	try {
		const { values, positionals } = strictArguments(
			process.argv.slice(2),
			{ help: { type: 'boolean' } },
			true,
		);
		if (values.help)
			process.stdout.write(
				'showcase/qualify-transport.ts PLAN.json: start the exact TS/Rust/wrapper STREAM pair, then stop before the example spawns; retain per-mode lifecycle proof.\n',
			);
		else
			await qualifyShowcaseTransport(showcasePlanSchema.parse(readJson(planArgument(positionals))));
	} catch (error) {
		reportCliFailure(error);
	}
