/** Compiled terminal journeys: real Rust-owned PTY bytes -> libghostty cells.
 * node scripts/verification/tui/capture-run.ts PLAN.json [--check]
 */
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { z } from 'zod';

import { isMain, reportCliFailure, strictArguments, planArgument } from './cli-arguments.ts';
import { GhosttyJourneyInput, requireDelivered } from './ghostty-journey-input.ts';
import { checkGridControls } from './grid-control-checks.ts';
import { gridFieldPair, observeGridScene } from './grid-scene-matcher.ts';
import { journeyCatalogue } from './journey-catalogue.ts';
import { journeyDefinitionSchema } from './journey-definition.ts';
import type { JourneyAction } from './journey-definition.ts';
import { encodeJourneyKey } from './journey-key.ts';
import { PtyGridDriver } from './pty-grid-driver.ts';
import type { KeyProtocol } from './pty-grid-driver.ts';
import { canonicalJson, sourceBinding } from './source-binding.ts';
import { gridLines } from './terminal-grid.ts';
import { renderOriginal } from './terminal-original.ts';
import { qualifyTerminal } from './terminal-qualification.ts';

const artifactSchema = z.strictObject({
	path: z.string(),
	sha256: z.string().regex(/^[a-f0-9]{64}$/u),
});
export const gridCapturePlanSchema = z.strictObject({
	repository: z.string(),
	sourceHead: z.string(),
	driver: artifactSchema,
	product: artifactSchema,
	component: artifactSchema.optional(),
	output: z.string(),
	journey: z.string(),
	keys: z.enum(['legacy', 'kitty']),
	chrome: z.string().optional(),
	inputProfile: z.enum(['terminal-only', 'example-ghostty-keybinds']).optional(),
});
export type GridCapturePlan = z.infer<typeof gridCapturePlanSchema>;
async function applyAction(
	driver: PtyGridDriver,
	action: JourneyAction,
	keys: KeyProtocol,
	input?: GhosttyJourneyInput,
): Promise<void> {
	if (action.kind === 'key') {
		const before = driver.frameGeneration();
		if (input !== undefined) {
			await Array.from({ length: action.repeat }).reduce<Promise<void>>(async (previous) => {
				await previous;
				requireDelivered(await input.key(action.key));
			}, Promise.resolve());
		} else {
			await driver.send(
				Buffer.concat(
					Array.from({ length: action.repeat }, () => encodeJourneyKey(action.key, keys)),
				),
			);
		}
		if (action.repeat === 1 && action.key !== 'Ctrl+D')
			await driver.waitForGrid(`frame after ${action.key}`, () => true, before);
	} else if (action.kind === 'text') {
		const before = driver.frameGeneration();
		if (input === undefined) await driver.send(Buffer.from(action.text));
		else requireDelivered(await input.text(action.text));
		if (action.text !== 'q') await driver.waitForGrid('frame after text', () => true, before);
	} else if (action.kind === 'field') {
		await driver.waitForGrid(`${action.dialog} ${action.label} ${action.value}`, (grid) =>
			gridFieldPair(grid, action.dialog, action.label, action.value),
		);
	} else {
		const expression = new RegExp(action.pattern, 'su');
		await driver.waitForGrid(`intermediate ${action.pattern}`, (grid) =>
			expression.test(gridLines(grid).join('\n')),
		);
	}
}
async function applyActions(
	driver: PtyGridDriver,
	actions: readonly JourneyAction[],
	keys: KeyProtocol,
	input?: GhosttyJourneyInput,
): Promise<void> {
	await actions.reduce<Promise<void>>(async (previous, action) => {
		await previous;
		await applyAction(driver, action, keys, input);
	}, Promise.resolve());
}
const requireTest = (binary: string, name: string): void => {
	const listing = execFileSync(binary, [name, '--exact', '--list'], { encoding: 'utf8' });
	if (!listing.split('\n').includes(`${name}: test`))
		throw new Error(`declared test entry absent: ${name}`);
};
export async function checkGridCapture(plan: GridCapturePlan): Promise<void> {
	if (sourceBinding(plan.repository).head !== plan.sourceHead)
		throw new Error('declared source head changed');
	if (journeyCatalogue[plan.journey] === undefined)
		throw new Error(`unknown journey ${plan.journey}`);
	for (const artifact of [
		plan.driver,
		plan.product,
		...(plan.component === undefined ? [] : [plan.component]),
	]) {
		const actual = createHash('sha256').update(readFileSync(artifact.path)).digest('hex');
		if (actual !== artifact.sha256) throw new Error(`declared build changed: ${artifact.path}`);
		execFileSync('/usr/bin/codesign', ['--verify', '--strict', artifact.path]);
	}

	requireTest(plan.driver.path, 'compiled_grid_terminal_observation');
	if (plan.journey.startsWith('normal-') || plan.journey.startsWith('hard-')) {
		if (plan.component === undefined) throw new Error('component fixture required');
		requireTest(
			plan.component.path,
			'quota_browse_presentation_test::quota_storyboard_fixture_test::compiled_quota_component_storyboard_fixture',
		);
	}
	await qualifyTerminal();
}
export async function runGridCapture(plan: GridCapturePlan): Promise<void> {
	const journey = journeyDefinitionSchema.parse(journeyCatalogue[plan.journey]);
	const dimensions = /-(\d+)x(\d+)$/u.exec(journey.name);
	if (dimensions === null) throw new Error('journey dimensions missing');
	const columns = Number(dimensions[1]);
	const rows = Number(dimensions[2]);
	const category = journey.name.split('-')[0];
	const surface =
		category === 'sessions'
			? 'sessions-filters'
			: category === 'quota'
				? 'quota-filters'
				: category === 'normal'
					? 'normal'
					: 'hard';
	if ((surface === 'normal' || surface === 'hard') && plan.component === undefined)
		throw new Error('component fixture build required');
	mkdirSync(plan.output);
	writeFileSync(join(plan.output, 'declared-target.json'), JSON.stringify(plan, null, 2));
	const source = sourceBinding(plan.repository);
	writeFileSync(join(plan.output, 'source-binding.json'), JSON.stringify(source, null, 2));
	const driver = await PtyGridDriver.start({
		repository: plan.repository,
		driver: plan.driver.path,
		surface,
		keys: plan.keys,
		evidence: plan.output,
		...(plan.component === undefined ? {} : { componentBinary: plan.component.path }),
	});
	const checkpoints: unknown[] = [];
	const input =
		plan.inputProfile === undefined
			? undefined
			: new GhosttyJourneyInput(driver, plan.inputProfile);
	try {
		await driver.waitForGrid('compiled initial screen', (grid) =>
			gridLines(grid).some((line) =>
				line.includes(surface === 'sessions-filters' ? 'Sessions' : 'Quota status'),
			),
		);
		if (columns !== 120 || rows !== 36) await driver.resize(columns, rows);
		await journey.groups.reduce<Promise<void>>(async (previous, group) => {
			await previous;
			await applyActions(driver, group.actions, plan.keys, input);
			const grid = await driver.waitForGrid(
				group.checkpoint,
				(candidate) => observeGridScene(candidate, group.checkpoint).passed,
			);
			const observation = observeGridScene(grid, group.checkpoint);
			const bytes = driver.bytes();
			writeFileSync(join(plan.output, `${group.checkpoint}.ansi`), bytes);
			writeFileSync(
				join(plan.output, `${group.checkpoint}.grid.json`),
				JSON.stringify(grid, null, 2),
			);
			writeFileSync(
				join(plan.output, `${group.checkpoint}.txt`),
				`${gridLines(grid).join('\n')}\n`,
			);
			writeFileSync(
				join(plan.output, `${group.checkpoint}.observation.json`),
				JSON.stringify(observation, null, 2),
			);
			const negativeControls = !observeGridScene(
				{
					...grid,
					cells: grid.cells.map((row) =>
						row.map((cell) => ({ ...cell, char: 32, chars: ' ', width: 1 })),
					),
				},
				group.checkpoint,
			).passed;
			if (!negativeControls) throw new Error('blank grid unexpectedly passed');
			if (plan.chrome !== undefined)
				await renderOriginal({
					chrome: plan.chrome,
					output: plan.output,
					bytes,
					grid,
					resizes: driver.resizeHistory(),
					columns,
					rows,
					checkpoint: group.checkpoint,
					sourceHead: plan.sourceHead,
					binarySha256:
						surface === 'normal' || surface === 'hard'
							? (plan.component?.sha256 ?? '')
							: plan.product.sha256,
				});
			checkpoints.push({
				checkpoint: group.checkpoint,
				ptySha256: createHash('sha256').update(bytes).digest('hex'),
				passed: observation.passed,
				blankRejected: negativeControls,
			});
		}, Promise.resolve());
		await applyActions(driver, journey.finish, plan.keys, input);
		const drain = async (): Promise<void> => {
			if ((await driver.receive()) !== 'eof') await drain();
		};
		await drain();
		const code = await driver.finish();
		if (code !== 0) throw new Error(`driver exited ${code}`);
		const controls = await checkGridControls(plan.output, plan.journey);
		if (canonicalJson(sourceBinding(plan.repository)) !== canonicalJson(source))
			throw new Error('source changed during observation');
		for (const artifact of [
			plan.driver,
			plan.product,
			...(plan.component === undefined ? [] : [plan.component]),
		]) {
			if (
				createHash('sha256').update(readFileSync(artifact.path)).digest('hex') !== artifact.sha256
			)
				throw new Error('declared build changed during observation');
		}
		writeFileSync(
			join(plan.output, 'journey-receipt.json'),
			JSON.stringify(
				{
					sourceHead: plan.sourceHead,
					viewport: { columns, rows },
					keys: plan.keys,
					journey: plan.journey,
					checkpoints,
					wrongSceneControls: controls.length,
					sourceBinding: 'source-binding.json',
					childExit: 0,
					fixtureEffects: 'owning bridge checked',
					originalImages: plan.chrome !== undefined,
				},
				null,
				2,
			),
		);
	} finally {
		await driver.stop();
		if (input !== undefined)
			writeFileSync(
				join(plan.output, 'input-delivery.json'),
				JSON.stringify(
					{
						profile: plan.inputProfile,
						records: input.history(),
						limits: [
							'physical keyboard hardware',
							'Ghostty app keybinds beyond the modelled set',
							'OS-level shortcuts',
							'real Ghostty rendering beyond libghostty',
						],
					},
					null,
					2,
				),
			);
		writeFileSync(
			join(plan.output, 'checkpoint-progress.json'),
			JSON.stringify(checkpoints, null, 2),
		);
	}
}
export async function captureMain(args: readonly string[]): Promise<void> {
	const { values, positionals } = strictArguments(
		args,
		{ help: { type: 'boolean' }, check: { type: 'boolean' }, showcase: { type: 'boolean' } },
		true,
	);
	if (values.help) {
		process.stdout.write(
			'capture-run.ts PLAN.json [--check]: exact compiled fixture, key mode, viewport, libghostty cell assertions and optional original PNGs via Chrome DevTools. Use --showcase with a component-showcase plan for SC01–SC27; prepare one with showcase/prepare-plan.ts.\n',
		);
		return;
	}
	if (values.showcase) {
		const { runShowcaseJourneys } = await import('./showcase/run.ts');
		await runShowcaseJourneys(
			JSON.parse(readFileSync(planArgument(positionals), 'utf8')),
			values.check === true,
		);
		return;
	}
	const plan = gridCapturePlanSchema.parse(
		JSON.parse(readFileSync(planArgument(positionals), 'utf8')),
	);
	await checkGridCapture(plan);
	if (values.check)
		process.stdout.write('declared artifacts and emulator qualified; no product observation\n');
	else await runGridCapture(plan);
}
if (isMain(import.meta.url)) await captureMain(process.argv.slice(2)).catch(reportCliFailure);
