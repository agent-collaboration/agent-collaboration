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
	MAX_ROUTE_PREFIX_BYTES,
	MAX_ROUTE_SEGMENT_BYTES,
	captureClaudeRouteBaseline,
	requireClaudeRouteBinding,
} from '../../../verification/observations/claude-route-window.ts';
import type { ClaudeRouteBinding } from '../../../verification/observations/claude-route-window.ts';
import {
	codexInformationCommandSchema,
	informationJourney,
} from './codex-acp-information-scenes.ts';
import { codexMenuKeySchema, modelJourney } from './codex-acp-model-scenes.ts';
import { codexOrdinaryPromptSchema, promptJourney } from './codex-acp-prompt-scenes.ts';
import { refreshJourney } from './codex-acp-refresh-scenes.ts';
import {
	CODEX_QUIT_STEP,
	UI_CHECKPOINT_MS,
	codexFixtureTitleSchema,
	compactJourney,
	encodeCodexAction,
	gridShowsCheckpoint,
	interruptJourney,
	pickerOnlyJourney,
	renameJourney,
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
		// A Claude prompt or selected-skill route proof only: the declared normal debug Host's run
		// directory (its router-audit.jsonl and debug-host-context.json), the same Host process, and
		// the byte bounds of the audit prefix hashed before Enter and of the segment read after it.
		claude_route: z
			.strictObject({
				host_run_directory: absolutePathSchema,
				host_pid: z.number().int().positive(),
				max_prefix_bytes: z.number().int().positive().max(MAX_ROUTE_PREFIX_BYTES),
				max_segment_bytes: z.number().int().positive().max(MAX_ROUTE_SEGMENT_BYTES),
			})
			.optional(),
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
			// The refreshed catalogue page and stale bootstrap rows from the actual bound and unbound
			// model/list, the selected model row and optional effort row, the apply, and its text.
			z.strictObject({
				kind: z.literal('model'),
				provider: providerSchema,
				catalogue_page: z.array(z.string().min(1)).min(1).max(8),
				stale_rows: z.array(z.string().min(1)),
				model_navigation: z.array(codexMenuKeySchema),
				target: z.strictObject({
					display_name: z.string().min(1),
					number: z.number().int().positive(),
					is_current: z.boolean(),
				}),
				effort: z
					.strictObject({
						navigation: z.array(codexMenuKeySchema),
						value: z.string().min(1),
						number: z.number().int().positive(),
						is_default: z.boolean(),
						is_current: z.boolean(),
					})
					.nullable(),
				apply: z.enum(['session', 'default']),
				expected_outcome: z.array(z.string().min(1)).min(1),
				outcome_deadline_ms: z.number().int().positive().max(600_000),
			}),
			// Diagnostic rename probe, not a delivery gate: a harmless fixture title and the endpoint's
			// declared refusal text (first entry is the error cell's leading text; each fits one row).
			z.strictObject({
				kind: z.literal('rename'),
				provider: providerSchema,
				title: codexFixtureTitleSchema,
				expected_refusal: z.array(z.string().min(1)).min(1),
			}),
			// One built-in read-only inventory command with no arguments; its refusal text is Codex's own.
			z.strictObject({
				kind: z.literal('information'),
				provider: providerSchema,
				command: codexInformationCommandSchema,
			}),
			// One ordinary prompt and the completion line only its reply produces, within that bound.
			z.strictObject({
				kind: z.literal('prompt'),
				provider: providerSchema,
				prompt: codexOrdinaryPromptSchema,
				completion: z.string().min(8),
				reply_deadline_ms: z.number().int().positive().max(600_000),
			}),
			// One ordinary prompt kept running through /skills, the manage view and Esc, the completion
			// line only its own reply produces, and the bound for that reply.
			z.strictObject({
				kind: z.literal('refresh'),
				provider: providerSchema,
				prompt: codexOrdinaryPromptSchema,
				completion: z.string().min(8),
				reply_deadline_ms: z.number().int().positive().max(600_000),
			}),
		]),
	})
	.refine(
		(plan) =>
			plan.claude_route === undefined ||
			((plan.journey.kind === 'prompt' || plan.journey.kind === 'skill') &&
				plan.journey.provider === 'claude' &&
				plan.target.endpoint_id === 'claude-local'),
		'a Claude route proof is admitted only for a Claude prompt or selected-skill plan',
	)
	.readonly();
export type CodexAcpRunPlan = z.infer<typeof codexAcpRunPlanSchema>;

/** The composed-input checkpoint a route proof's baseline follows, or none without a proof. */
export function routeBaselineScene(
	plan: CodexAcpRunPlan,
): 'prompt-composed' | 'skill-input-composed' | null {
	if (plan.claude_route === undefined) return null;
	return plan.journey.kind === 'skill' ? 'skill-input-composed' : 'prompt-composed';
}

function claudeRouteBinding(plan: CodexAcpRunPlan): ClaudeRouteBinding | null {
	const route = plan.claude_route;
	if (route === undefined) return null;
	return {
		hostRunDirectory: route.host_run_directory,
		hostPid: route.host_pid,
		serviceDirectory: plan.target.service_directory,
		maxPrefixBytes: route.max_prefix_bytes,
		maxSegmentBytes: route.max_segment_bytes,
	};
}

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
	if (journey.kind === 'rename')
		return renameJourney(journey.provider, journey.title, journey.expected_refusal);
	if (journey.kind === 'information') return informationJourney(journey.provider, journey.command);
	if (journey.kind === 'prompt')
		return promptJourney({
			provider: journey.provider,
			prompt: journey.prompt,
			completion: journey.completion,
			replyDeadlineMs: journey.reply_deadline_ms,
		});
	if (journey.kind === 'refresh')
		return refreshJourney({
			provider: journey.provider,
			prompt: journey.prompt,
			completion: journey.completion,
			replyDeadlineMs: journey.reply_deadline_ms,
		});
	return modelJourney({
		provider: journey.provider,
		cataloguePage: journey.catalogue_page,
		staleRows: journey.stale_rows,
		modelNavigation: journey.model_navigation,
		target: {
			displayName: journey.target.display_name,
			number: journey.target.number,
			isCurrent: journey.target.is_current,
		},
		effort:
			journey.effort === null
				? null
				: {
						navigation: journey.effort.navigation,
						value: journey.effort.value,
						number: journey.effort.number,
						isDefault: journey.effort.is_default,
						isCurrent: journey.effort.is_current,
					},
		apply: journey.apply,
		expectedOutcome: journey.expected_outcome,
		outcomeDeadlineMs: journey.outcome_deadline_ms,
	});
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
	const binding = claudeRouteBinding(plan);
	if (binding !== null) {
		requireClaudeRouteBinding(binding);
		const observer = z
			.looseObject({ host_pid: z.number().int() })
			.parse(JSON.parse(readFileSync(plan.target.observer_plan, 'utf8')));
		if (observer.host_pid !== binding.hostPid)
			throw new Error('the route proof names another Host than its observer plan');
	}
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

/** The fixed route-proof hook: its composed-input scene and the record it takes there. */
export interface RouteBaselineHook {
	readonly afterScene: string;
	readonly capture: () => void;
}

/**
 * Drives every declared step in order and records each checkpoint as soon as it is drawn. A
 * declared Claude route baseline is taken at one fixed point: right after the composed-input
 * checkpoint's record (grid and original), while the next step, whose only action is the
 * submission Enter, has not been sent.
 */
export async function driveCodexJourney(
	driver: PtyGridDriver,
	journey: CodexJourney,
	keys: KeyProtocol,
	record: (step: CodexJourney['steps'][number], grid: TerminalGrid) => Promise<void>,
	routeBaseline: RouteBaselineHook | null,
): Promise<string[]> {
	const observed: string[] = [];
	await journey.steps.reduce<Promise<void>>(async (previous, step, index) => {
		await previous;
		const grid = await driveCodexStep(driver, step, keys);
		await record(step, grid);
		observed.push(step.checkpoint.scene);
		if (routeBaseline === null || step.checkpoint.scene !== routeBaseline.afterScene) return;
		const submission = journey.steps[index + 1]?.actions;
		if (JSON.stringify(submission) !== JSON.stringify([{ kind: 'key', key: 'Enter' }]))
			throw new Error('a Claude route baseline must directly precede the submission Enter');
		routeBaseline.capture();
	}, Promise.resolve());
	return observed;
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
	let observed: string[] = [];
	const originals: unknown[] = [];
	let child: ProcessIdentity | null = null;
	const binding = claudeRouteBinding(plan);
	const afterScene = routeBaselineScene(plan);
	const routeBaseline =
		binding === null || afterScene === null
			? null
			: {
					afterScene,
					capture: (): void => {
						const baseline = captureClaudeRouteBaseline(binding, {
							afterScene,
							providerSessionId: plan.target.provider_session_id,
							nativeThreadId: plan.target.native_thread_id,
						});
						writeFileSync(
							join(plan.output, 'claude-route-baseline.json'),
							JSON.stringify(baseline, null, 2),
							{ mode: 0o600, flag: 'wx' },
						);
					},
				};
	try {
		if (columns !== 120 || rows !== 36) await driver.resize(columns, rows);
		const record = async (
			step: CodexJourney['steps'][number],
			grid: TerminalGrid,
		): Promise<void> => {
			const { checkpoint } = step;
			const current = codexChild(plan.output, plan.target);
			if (child !== null && !sameIdentity(current, child))
				throw new Error('Codex PTY child was replaced during the journey');
			child = current;
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
		};
		observed = await driveCodexJourney(driver, journey, plan.keys, record, routeBaseline);
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
