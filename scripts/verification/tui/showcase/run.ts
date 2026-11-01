/** Build-and-drive observations only; independent acceptance belongs to the Advisor. */
import { execFileSync } from 'node:child_process';
import { mkdirSync, readFileSync, realpathSync, writeFileSync } from 'node:fs';
import { isAbsolute, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { isMain, planArgument, reportCliFailure, strictArguments } from '../cli/cli-arguments.ts';
import { canonicalJson, fileDigest, sourceBinding } from '../evidence/source-binding.ts';
import { qualifyTerminal } from '../terminal/terminal-qualification.ts';
import { PrerequisiteGap, ScenarioMismatch, showcasePlanSchema } from './contracts.ts';
import type { ShowcasePlan } from './contracts.ts';
import { ShowcaseRig } from './rig.ts';
import { scenarioMatrix } from './scenario-matrix.ts';
import type { ScenarioMatrixEntry } from './scenario-matrix.ts';
import { variantLabel } from './scenario-variants.ts';
import { driveScenario, scenarioTitles } from './scenarios.ts';
interface CaseResult extends ScenarioMatrixEntry {
	readonly title: string;
	readonly keys: 'legacy' | 'kitty';
	readonly columns: number;
	readonly rows: number;
	readonly status: 'pass' | 'fail' | 'blocked-by-prerequisite' | 'unobserved';
	readonly evidence: string;
	readonly reason: string | null;
	readonly observed: unknown;
	readonly cleanupError: string | null;
}
export async function checkShowcaseTarget(plan: ShowcasePlan): Promise<void> {
	if (realpathSync(fileURLToPath(new URL('../../../../', import.meta.url))) !== plan.repository)
		throw new Error('showcase runner must come from the declared apparatus checkout');
	if (plan.chrome === undefined)
		throw new PrerequisiteGap(
			'showcase observations require nominated Chrome for original wterm PNGs',
		);
	if (!isAbsolute(plan.output) || !isAbsolute(plan.repository))
		throw new Error('showcase paths must be absolute');
	if (canonicalJson(sourceBinding(plan.repository)) !== canonicalJson(plan.source))
		throw new Error('declared apparatus source changed');
	if (
		canonicalJson(sourceBinding(plan.exampleSource.repository)) !==
		canonicalJson(plan.exampleSource.source)
	)
		throw new Error('declared example source changed');
	if (process.version !== 'v26.12.1' || realpathSync(process.execPath) !== plan.node.path)
		throw new Error('observer must use its pinned Node executable');
	for (const artifact of [plan.driver, plan.example, plan.node])
		if (
			realpathSync(artifact.path) !== artifact.path ||
			fileDigest(artifact.path) !== artifact.sha256
		)
			throw new Error('declared artifact path/hash changed: ' + artifact.path);
	for (const artifact of [plan.driver, plan.example])
		execFileSync('/usr/bin/codesign', ['--verify', '--strict', artifact.path]);
	const entries = execFileSync(
		plan.driver.path,
		['quota_storyboard::compiled_grid_terminal_observation', '--exact', '--list'],
		{ encoding: 'utf8' },
	);
	if (!entries.includes('quota_storyboard::compiled_grid_terminal_observation: test'))
		throw new Error('declared grid driver entry missing');
	await qualifyTerminal();
}
export async function runShowcaseJourneys(value: unknown, checkOnly = false): Promise<void> {
	const plan = showcasePlanSchema.parse(value);
	const matrix = scenarioMatrix(plan);
	await checkShowcaseTarget(plan);
	if (checkOnly) return;
	mkdirSync(plan.output);
	writeFileSync(join(plan.output, 'declared-target.json'), JSON.stringify(plan, null, 2));
	const results: CaseResult[] = [];
	let cleanupUnsettled = false;
	let stopSignal: string | null = null;
	const stopInt = (): void => {
			stopSignal = 'SIGINT';
		},
		stopTerm = (): void => {
			stopSignal = 'SIGTERM';
		};
	process.on('SIGINT', stopInt);
	process.on('SIGTERM', stopTerm);
	const runEntry = async (entry: ScenarioMatrixEntry): Promise<void> => {
		if (cleanupUnsettled) return;
		const evidence = join(
			plan.output,
			`${entry.scenarioId}-${entry.keys}-${entry.columns}x${entry.rows}${entry.variant.kind === 'ordinary' ? '' : '-' + variantLabel(entry.variant)}`,
		);
		let rig: ShowcaseRig | null = null;
		let status: CaseResult['status'] = 'unobserved',
			reason: string | null = null,
			observed: unknown = null,
			cleanupError: string | null = null;
		try {
			rig = await ShowcaseRig.start({ plan, directory: evidence, ...entry });
			await driveScenario(entry.scenarioId, rig, entry.variant);
			await rig.capture('final-observed');
			await rig.quit();
			status = 'pass';
		} catch (error) {
			status =
				error instanceof ScenarioMismatch
					? 'fail'
					: error instanceof PrerequisiteGap
						? 'blocked-by-prerequisite'
						: 'unobserved';
			reason = error instanceof Error ? error.message : String(error);
			if (error instanceof ScenarioMismatch) observed = error.observed;
			if (rig !== null) {
				try {
					await rig.capture('last-observed');
				} catch (captureError) {
					observed = { observation: observed, captureError: String(captureError) };
				}
			}
		} finally {
			if (rig !== null)
				try {
					await rig.stop();
				} catch (error) {
					cleanupError = String(error);
					cleanupUnsettled = true;
				}
		}
		results.push({
			...entry,
			title: scenarioTitles[entry.scenarioId],
			status,
			evidence,
			reason,
			observed,
			cleanupError,
		});
		results.sort((left, right) => left.ordinal - right.ordinal);
		writeFileSync(
			join(plan.output, 'results.json'),
			JSON.stringify(
				{
					kind: 'component-showcase-build-and-drive',
					source: plan.exampleSource.source.head,
					apparatusSource: plan.source,
					example: plan.example,
					driver: plan.driver,
					independentVerdict: false,
					expectedCases: matrix.length,
					concurrency: 2,
					stopSignal,
					cleanupUnsettled,
					results,
				},
				null,
				2,
			),
		);
		process.stdout.write(
			`${entry.scenarioId} ${entry.keys} ${entry.columns}x${entry.rows} ${variantLabel(entry.variant)} ${status}${reason === null ? '' : ': ' + reason}\n`,
		);
	};
	let nextCase = 0;
	const observeNext = async (): Promise<void> => {
		if (cleanupUnsettled || stopSignal !== null) return;
		const entry = matrix[nextCase];
		if (entry === undefined) return;
		nextCase += 1;
		await runEntry(entry);
		await observeNext();
	};
	let workerFailures: readonly PromiseSettledResult<void>[] = [];
	try {
		workerFailures = await Promise.allSettled([observeNext(), observeNext()]);
	} finally {
		process.off('SIGINT', stopInt);
		process.off('SIGTERM', stopTerm);
	}
	for (const result of workerFailures) if (result.status === 'rejected') throw result.reason;

	if (canonicalJson(sourceBinding(plan.repository)) !== canonicalJson(plan.source))
		throw new Error('source changed during showcase observation');
	if (
		canonicalJson(sourceBinding(plan.exampleSource.repository)) !==
		canonicalJson(plan.exampleSource.source)
	)
		throw new Error('example source changed during showcase observation');
	for (const artifact of [plan.driver, plan.example, plan.node])
		if (
			realpathSync(artifact.path) !== artifact.path ||
			fileDigest(artifact.path) !== artifact.sha256
		)
			throw new Error('artifact changed during showcase observation');
	const table = [
		'| SC | Mode | Grid | Variant | Result | Reason |',
		'| --- | --- | --- | --- | --- | --- |',
		...results.map(
			(row) =>
				`| ${row.scenarioId} | ${row.keys} | ${row.columns}x${row.rows} | ${variantLabel(row.variant)} | ${row.status} | ${(row.reason ?? 'observed; independent verdict pending').replaceAll('|', '/').replaceAll('\n', ' ')} |`,
		),
	];
	writeFileSync(join(plan.output, 'results.md'), table.join('\n') + '\n');
	if (
		cleanupUnsettled ||
		stopSignal !== null ||
		results.length !== matrix.length ||
		results.some((row) => row.status !== 'pass')
	)
		throw new Error(
			'showcase observation includes failures or unmet prerequisites; inspect the per-SC table',
		);
}
export async function showcaseMain(args: readonly string[]): Promise<void> {
	const { values, positionals } = strictArguments(
		args,
		{ help: { type: 'boolean' }, check: { type: 'boolean' } },
		true,
	);
	if (values.help) {
		process.stdout.write(
			'showcase/run.ts PLAN.json [--check]: fixed public-wrapper sample, exact input/state/frame receipts and per-SC observations. Default matrix is all 27 IDs, four grids, both modes.\n',
		);
		return;
	}
	await runShowcaseJourneys(
		JSON.parse(readFileSync(planArgument(positionals), 'utf8')),
		values.check === true,
	);
}
if (isMain(import.meta.url)) await showcaseMain(process.argv.slice(2)).catch(reportCliFailure);
