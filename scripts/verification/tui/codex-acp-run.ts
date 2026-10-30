/**
 * `capture-run.ts --codex-acp-only --codex-acp-plan PLAN.json [--check]`: one declared Codex ACP
 * journey in the unchanged installed Codex TUI, on the shared Rust TerminalDriver and wterm grid.
 * It qualifies the target before launch, renders same-byte original PNGs per checkpoint, requires
 * one unchanged Codex child and the declared checkpoint order, then exits Codex normally. Provider
 * effects, history and settings are decided separately by the same-session observer
 * (verification/observations/acp-journey-observation.ts); this run never claims them.
 */
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, realpathSync, writeFileSync } from 'node:fs';
import { isAbsolute, join } from 'node:path';

import { z } from 'zod';

import {
	CODEX_QUIT_STEP,
	UI_CHECKPOINT_MS,
	codexActionSchema,
	compactJourney,
	encodeCodexAction,
	gridShowsCheckpoint,
	interruptJourney,
	modelJourney,
	pickerOnlyJourney,
	reopenJourney,
	requireCheckpointSequence,
	skillJourney,
} from './codex-acp-scenes.ts';
import type { CodexJourney } from './codex-acp-scenes.ts';
import { codexAcpTargetSchema, qualifyCodexAcpTarget } from './codex-acp-target.ts';
import type { CodexAcpTarget } from './codex-acp-target.ts';
import { artifactBindingSchema } from './evidence-schema.ts';
import type { ProcessIdentity } from './evidence-schema.ts';
import { lookupProcessIdentity, sameIdentity } from './process-identity.ts';
import { PtyGridDriver } from './pty-grid-driver.ts';
import type { KeyProtocol } from './pty-grid-driver.ts';
import { sourceBinding, verifyArtifact } from './source-binding.ts';
import { gridLines } from './terminal-grid.ts';
import type { TerminalGrid } from './terminal-grid.ts';
import { renderOriginal } from './terminal-original.ts';
import { qualifyTerminal } from './terminal-qualification.ts';

const absolutePathSchema = z.string().refine(isAbsolute, 'absolute path required');
const providerSchema = z.enum(['claude', 'cursor']);
export const codexAcpRunPlanSchema = z
	.strictObject({
		repository: absolutePathSchema,
		source_head: z.string().regex(/^[a-f0-9]{40}$/u),
		// The signed quota_storyboard_capture_test binary that owns the PTY.
		driver: artifactBindingSchema,
		node: artifactBindingSchema,
		chrome: artifactBindingSchema,
		keys: z.enum(['legacy', 'kitty']),
		output: absolutePathSchema,
		target: codexAcpTargetSchema,
		journey: z.discriminatedUnion('kind', [
			z.strictObject({
				kind: z.literal('skill'),
				provider: providerSchema,
				skills: z.array(z.string().min(1)).min(1),
				prefix: z.string(),
				args: z.string().min(1),
				expected_effects: z.array(z.string().min(1)).min(1),
				image_path: absolutePathSchema.nullable(),
				effect_deadline_ms: z.number().int().positive().max(600_000),
			}),
			z.strictObject({
				kind: z.literal('compact'),
				provider: providerSchema,
				effect_deadline_ms: z.number().int().positive().max(600_000),
			}),
			z.strictObject({
				kind: z.literal('interrupt'),
				provider: providerSchema,
				prompt: z.string().min(1),
			}),
			// R1 without model input: the selected picker row, then the draft cleared.
			z.strictObject({
				kind: z.literal('picker'),
				provider: providerSchema,
				skill: z.string().min(1),
			}),
			// The reply this Session already holds, shown again on resume with no new prompt.
			z.strictObject({
				kind: z.literal('reopen'),
				provider: providerSchema,
				prior_reply: z.array(z.string().min(1)).min(1),
			}),
			// Keys moving to one advertised model row, how it is applied, and the text Codex shows.
			z.strictObject({
				kind: z.literal('model'),
				provider: providerSchema,
				navigation: z.array(codexActionSchema),
				apply: z.enum(['session', 'default']),
				expected_outcome: z.array(z.string().min(1)).min(1),
				outcome_deadline_ms: z.number().int().positive().max(600_000),
			}),
		]),
	})
	.readonly();
export type CodexAcpRunPlan = z.infer<typeof codexAcpRunPlanSchema>;

export function codexJourneyFor(plan: CodexAcpRunPlan): CodexJourney {
	const journey = plan.journey;
	if (journey.provider !== (plan.target.endpoint_id === 'claude-local' ? 'claude' : 'cursor'))
		throw new Error('journey provider differs from the target endpoint');
	if (journey.kind === 'skill')
		return skillJourney({
			provider: journey.provider,
			skills: journey.skills,
			prefix: journey.prefix,
			args: journey.args,
			expectedEffects: journey.expected_effects,
			imagePath: journey.image_path,
			effectDeadlineMs: journey.effect_deadline_ms,
		});
	if (journey.kind === 'compact')
		return compactJourney(journey.provider, journey.effect_deadline_ms);
	if (journey.kind === 'interrupt') return interruptJourney(journey.provider, journey.prompt);
	if (journey.kind === 'picker') return pickerOnlyJourney(journey.provider, journey.skill);
	if (journey.kind === 'reopen') return reopenJourney(journey.provider, journey.prior_reply);
	return modelJourney(
		journey.provider,
		journey.navigation,
		journey.apply,
		journey.expected_outcome,
		journey.outcome_deadline_ms,
	);
}

/** Everything a run needs, checked without launching Codex. */
export async function checkCodexAcp(plan: CodexAcpRunPlan): Promise<CodexJourney> {
	if (sourceBinding(plan.repository).head !== plan.source_head)
		throw new Error('declared source head changed');
	for (const artifact of [plan.driver, plan.node, plan.chrome]) verifyArtifact(artifact);
	execFileSync('/usr/bin/codesign', ['--verify', '--strict', plan.driver.path]);
	const listing = execFileSync(
		plan.driver.path,
		['compiled_codex_acp_grid_observation', '--exact', '--list', '--ignored'],
		{ encoding: 'utf8' },
	);
	if (!listing.split('\n').includes('compiled_codex_acp_grid_observation: test'))
		throw new Error('declared Codex ACP test entry absent');
	if (realpathSync(process.execPath) !== plan.node.path || process.version !== 'v26.12.1')
		throw new Error('the run must use the declared Node v26.12.1');
	await qualifyTerminal();
	qualifyCodexAcpTarget(plan.target);
	return codexJourneyFor(plan);
}

/** The live PTY child: the declared installed Codex, owned by this user (real UID). */
export function codexChild(output: string, target: CodexAcpTarget): ProcessIdentity {
	const identity = lookupProcessIdentity(
		Number(readFileSync(join(output, 'quota-child.pid'), 'utf8').trim()),
	);
	if (
		identity === null ||
		identity.realUid !== process.getuid?.() ||
		realpathSync(identity.executable) !== target.codex.path
	)
		throw new Error('PTY child is not the declared installed Codex owned by this user');
	return identity;
}

/** Writes one step's actions in order (one write each), then waits for its checkpoint frame. */
export async function driveCodexStep(
	driver: PtyGridDriver,
	step: CodexJourney['steps'][number],
	keys: KeyProtocol,
): Promise<TerminalGrid> {
	const after = driver.frameGeneration();
	await step.actions.reduce<Promise<void>>(async (sent, action) => {
		await sent;
		await driver.send(encodeCodexAction(action, keys));
	}, Promise.resolve());
	const { checkpoint } = step;
	return driver.waitForGrid(
		checkpoint.scene,
		(candidate) => gridShowsCheckpoint(gridLines(candidate), checkpoint),
		after,
		checkpoint.deadline_ms,
	);
}

/** Codex's own normal exit: `/quit` once its command row is ready, Enter, then PTY EOF. */
export async function exitCodex(driver: PtyGridDriver, keys: KeyProtocol): Promise<void> {
	await driveCodexStep(driver, CODEX_QUIT_STEP, keys);
	await driver.send(encodeCodexAction({ kind: 'key', key: 'Enter' }, keys));
	await driver.waitForEof('codex /quit', UI_CHECKPOINT_MS);
}

export async function runCodexAcp(plan: CodexAcpRunPlan): Promise<void> {
	const journey = await checkCodexAcp(plan);
	if (existsSync(plan.output)) throw new Error('fresh Codex ACP output required');
	mkdirSync(plan.output, { mode: 0o700 });
	writeFileSync(join(plan.output, 'declared-plan.json'), JSON.stringify(plan, null, 2));
	const config = join(plan.output, 'codex-target.json');
	const configuration = {
		surface: 'codex-acp',
		binary: plan.target.codex.path,
		binary_sha256: plan.target.codex.sha256,
		target: JSON.stringify(plan.target),
		output: plan.output,
	};
	writeFileSync(config, JSON.stringify(configuration), { mode: 0o600, flag: 'wx' });
	const driver = await PtyGridDriver.start({
		repository: plan.repository,
		driver: plan.driver.path,
		surface: 'codex-acp',
		keys: plan.keys,
		evidence: plan.output,
		publicEnvironment: {
			QUOTA_STORYBOARD_NODE: plan.node.path,
			QUOTA_STORYBOARD_NODE_SHA256: plan.node.sha256,
			TUI_GRID_CODEX_OUTPUT: plan.output,
			TUI_GRID_CODEX_CONFIG: config,
			TUI_GRID_CODEX_CONFIG_SHA256: createHash('sha256').update(readFileSync(config)).digest('hex'),
		},
	});
	const { columns, rows } = plan.target.viewport;
	const observed: string[] = [];
	const originals: unknown[] = [];
	let child: ProcessIdentity | null = null;
	try {
		if (columns !== 120 || rows !== 36) await driver.resize(columns, rows);
		await journey.steps.reduce<Promise<void>>(async (previous, step) => {
			await previous;
			const grid = await driveCodexStep(driver, step, plan.keys);
			const { checkpoint } = step;
			const current = codexChild(plan.output, plan.target);
			if (child !== null && !sameIdentity(current, child))
				throw new Error('Codex PTY child was replaced during the journey');
			child = current;
			observed.push(checkpoint.scene);
			writeFileSync(join(plan.output, `${checkpoint.scene}.grid.json`), JSON.stringify(grid));
			originals.push(
				await renderOriginal({
					chrome: plan.chrome.path,
					output: plan.output,
					bytes: driver.bytes(),
					grid,
					resizes: driver.resizeHistory(),
					columns,
					rows,
					checkpoint: checkpoint.scene,
					sourceHead: plan.source_head,
					binarySha256: plan.target.codex.sha256,
				}),
			);
		}, Promise.resolve());
		requireCheckpointSequence(observed, journey);
		await exitCodex(driver, plan.keys);
		if ((await driver.finish()) !== 0) throw new Error('Codex ACP driver failed');
	} finally {
		await driver.stop();
	}
	writeFileSync(
		join(plan.output, 'codex-acp-run.json'),
		JSON.stringify(
			{
				journey: journey.name,
				checkpoints: observed,
				child,
				originals,
				complete: false,
				limits:
					'TUI scenes only; provider effect, history and settings need the same-session observer.',
			},
			null,
			2,
		),
	);
}

export async function codexAcpMain(planPath: string, checkOnly: boolean): Promise<void> {
	const plan = codexAcpRunPlanSchema.parse(JSON.parse(readFileSync(planPath, 'utf8')));
	if (checkOnly) {
		await checkCodexAcp(plan);
		process.stdout.write('declared Codex ACP target, driver and emulator qualified; no TUI run\n');
		return;
	}
	await runCodexAcp(plan);
	process.stdout.write(`${join(plan.output, 'codex-acp-run.json')}\n`);
}
