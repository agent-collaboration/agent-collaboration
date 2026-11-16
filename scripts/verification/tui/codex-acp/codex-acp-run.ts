/**
 * `capture-run.ts --codex-acp-only --codex-acp-plan PLAN.json [--check]`: one declared Codex ACP
 * journey in the unchanged installed Codex TUI, on the shared Rust TerminalDriver and wterm grid.
 * It qualifies the target before launch, renders same-byte original PNGs per checkpoint, requires
 * one unchanged Codex child and the declared checkpoint order, then exits Codex normally. The
 * ordinary-prompt input, turn, terminal outcome and reply are read back through ordinary MCP.
 * Other effects, history and settings still need the same-session ACP observer.
 */
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, realpathSync, writeFileSync } from 'node:fs';
import { isAbsolute, join } from 'node:path';

import { z } from 'zod';

import { declaredProviderSelectionSchema } from '../../../../verification/observations/mcp-control/observations/declared-model.ts';
import type { DeclaredProviderSelection } from '../../../../verification/observations/mcp-control/observations/declared-model.ts';
import { normalBuildManifestSchema } from '../../../../verification/observations/providers/acp/acp-metadata-models.ts';
import { RUN_RECORD_FILE } from '../../../../verification/observations/providers/acp/codex-compare-evaluation.ts';
import type { CompareRunRecord } from '../../../../verification/observations/providers/acp/codex-compare-evaluation.ts';
import { CodexWireTee } from '../../../../verification/observations/providers/acp/codex-wire-tee.ts';
import {
	MAX_ROUTE_PREFIX_BYTES,
	MAX_ROUTE_SEGMENT_BYTES,
	captureClaudeRouteBaseline,
	requireClaudeRouteBinding,
} from '../../../../verification/observations/providers/claude/claude-route-window.ts';
import type { ClaudeRouteBinding } from '../../../../verification/observations/providers/claude/claude-route-window.ts';
import { artifactBindingSchema } from '../evidence/evidence-schema.ts';
import type { ProcessIdentity } from '../evidence/evidence-schema.ts';
import { sourceBinding, verifyArtifact } from '../evidence/source-binding.ts';
import { lookupProcessIdentity, sameIdentity } from '../processes/process-identity.ts';
import { PtyGridDriver } from '../terminal/pty-grid-driver.ts';
import type { KeyProtocol } from '../terminal/pty-grid-driver.ts';
import { gridLines } from '../terminal/terminal-grid.ts';
import type { TerminalGrid } from '../terminal/terminal-grid.ts';
import { renderOriginal } from '../terminal/terminal-original.ts';
import { qualifyTerminal } from '../terminal/terminal-qualification.ts';
import {
	approvalJourney,
	approvalJourneySchema,
	approvalWatcherJourney,
	scratchInsideOutput,
} from './codex-acp-approval-scenes.ts';
import { compareUsageReadback } from './codex-acp-compare-readback.ts';
import { compareJourney, compareJourneySchema } from './codex-acp-compare-scenes.ts';
import { requireLaunchComposer } from './codex-acp-composer-ready.ts';
import { settleWholeRunFrames } from './codex-acp-frame-observation.ts';
import {
	HistoricalRedContinuation,
	historicalRedSchema,
	requireRedsInJourney,
	runBuildSchema,
} from './codex-acp-historical-reds.ts';
import {
	codexInformationCommandSchema,
	informationJourney,
} from './codex-acp-information-scenes.ts';
import { cliMcpEndpoint, cliPromptReadback } from './codex-acp-mcp-readback.ts';
import type { PromptReadbackHooks } from './codex-acp-mcp-readback.ts';
import {
	codexSavedDefaultMessage,
	modelJourney,
	modelPickFields,
} from './codex-acp-model-scenes.ts';
import {
	approvalHooksFor,
	requireApprovalScratchBinding,
	requireFreshOutput,
	writeMovedRunRecord,
} from './codex-acp-moved-run.ts';
import {
	refusedJourney,
	refusedJourneySchema,
	resumeJourney,
	resumeJourneySchema,
	tierJourney,
	tierJourneySchema,
} from './codex-acp-moved-scenes.ts';
import { codexOrdinaryPromptSchema, promptJourney } from './codex-acp-prompt-scenes.ts';
import { referenceLaunchReadiness, referenceModelProof } from './codex-acp-reference-start.ts';
import { refreshJourney } from './codex-acp-refresh-scenes.ts';
import {
	CODEX_0_165_TEXT,
	CODEX_QUIT_STEP,
	UI_CHECKPOINT_MS,
	codexFixtureTitleSchema,
	compactJourney,
	encodeCodexAction,
	expectedErrorCells,
	gridShowsCheckpoint,
	interruptJourney,
	pickerOnlyJourney,
	renameJourney,
	reopenJourney,
	requireCheckpointSequence,
	skillJourney,
} from './codex-acp-scenes.ts';
import type { CodexJourney } from './codex-acp-scenes.ts';
import {
	codexAcpTargetSchema,
	codexSocketPath,
	qualifyCodexAcpTarget,
	routerSession,
} from './codex-acp-target.ts';
import type { CodexAcpEndpoint, CodexAcpTarget } from './codex-acp-target.ts';
import { referenceTurnFailure } from './codex-acp-turn-failure.ts';
import type { StepAbortCheck } from './codex-acp-turn-failure.ts';
import { wireTurnReading } from './codex-acp-turn-signal.ts';
import { attachWatchingWindow } from './codex-acp-windows-run.ts';
import type { WatchingWindow } from './codex-acp-windows-run.ts';
import {
	WINDOW_B_ATTACH_SCENE,
	windowsJourneySchema,
	windowsJourneys,
} from './codex-acp-windows-scenes.ts';

const absolutePathSchema = z.string().refine(isAbsolute, 'absolute path required');
// The ACP providers every journey kind but `compare` runs against; `compare` also admits real Codex.
const providerSchema = z.enum(['claude', 'cursor']);

/** Every endpoint's journey provider; a new endpoint fails to compile until it is mapped here. */
const PROVIDER_FOR_ENDPOINT = {
	'claude-local': 'claude',
	'cursor-local': 'cursor',
	'codex-local': 'codex',
} as const satisfies Record<CodexAcpEndpoint, 'claude' | 'cursor' | 'codex'>;
export const codexAcpRunPlanSchema = z
	.strictObject({
		repository: absolutePathSchema,
		source_head: z.string().regex(/^[a-f0-9]{40}$/u),
		// The signed e2e libtest with the namespaced ACP PTY entry.
		driver: artifactBindingSchema,
		node: artifactBindingSchema,
		chrome: artifactBindingSchema,
		keys: z.enum(['legacy', 'kitty']),
		output: absolutePathSchema,
		target: codexAcpTargetSchema,
		// A Claude prompt or selected-skill route proof only: the declared normal debug Host's run
		// directory (its router-audit.jsonl and debug-host-context.json), the same Host process, the
		// byte bounds of the audit prefix hashed before Enter and of the segment read after it, and
		// the normal-build collaboration CLI whose managed-Session inventory reads isolate the window.
		claude_route: z
			.strictObject({
				host_run_directory: absolutePathSchema,
				host_pid: z.number().int().positive(),
				max_prefix_bytes: z.number().int().positive().max(MAX_ROUTE_PREFIX_BYTES),
				max_segment_bytes: z.number().int().positive().max(MAX_ROUTE_SEGMENT_BYTES),
				collaboration_cli: artifactBindingSchema,
			})
			.optional(),
		// The build this run drives. Only a baseline run may name the steps the catalog expects to
		// fail there (assignment Correction 21); every other step still runs and is recorded.
		build: runBuildSchema.default('head'),
		historical_reds: z.array(historicalRedSchema).default([]),
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
			z
				.strictObject({
					kind: z.literal('model'),
					provider: providerSchema,
					...modelPickFields,
				})
				.refine(
					(model) =>
						model.apply === 'session'
							? model.expected_outcome.length > 0 && model.saved_default === null
							: model.expected_outcome.length === 0 && model.saved_default !== null,
					'a session apply declares its outcome; a default apply declares only what it saves',
				),
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
				selection: declaredProviderSelectionSchema,
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
			// O5: one action script run identically against real Codex and a Router target.
			compareJourneySchema,
			// Built in D0, run live in D3 (resume, refused, tier) and D4 (approval).
			resumeJourneySchema,
			refusedJourneySchema,
			tierJourneySchema,
			approvalJourneySchema,
			// Two concurrent windows on one thread (D3).
			windowsJourneySchema,
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
	.refine(
		(plan) =>
			plan.target.wire_tee === null || plan.target.wire_tee.recording.startsWith(`${plan.output}/`),
		'the wire tee recording belongs under the run output',
	)
	.refine(
		(plan) =>
			plan.journey.kind !== 'compare' ||
			(plan.target.wire_tee !== null && plan.target.status_line.length >= 2),
		'a comparison run records the wire and shows the model and context window in its footer',
	)
	.refine(
		(plan) =>
			plan.journey.kind !== 'compare' ||
			(plan.journey.initial_selection === null) === (plan.journey.provider === 'codex'),
		'a Router comparison target declares its initial selection; the real-Codex reference does not',
	)
	.refine(
		(plan) =>
			(plan.journey.kind !== 'resume' &&
				plan.journey.kind !== 'tier' &&
				plan.journey.kind !== 'approval' &&
				plan.journey.kind !== 'windows') ||
			plan.target.wire_tee !== null,
		'a resume, tier, approval or two-window run records the wire',
	)
	.refine(
		(plan) =>
			plan.journey.kind !== 'resume' ||
			plan.target.native_thread_id === null ||
			plan.journey.expected_thread_id !== plan.target.native_thread_id,
		'a resume run picks another thread than the one the CLI launched on',
	)
	.refine(
		(plan) =>
			plan.journey.kind !== 'approval' ||
			plan.target.endpoint_id === 'claude-local' ||
			plan.target.endpoint_id === 'codex-local',
		'an approval run needs a Claude Router Session or the real-Codex reference; Cursor has none',
	)
	.refine(
		(plan) =>
			plan.journey.kind !== 'approval' ||
			scratchInsideOutput(plan.journey.scratch_directory, plan.output),
		'the approval scratch directory lies inside the run output',
	)
	.refine(
		(plan) => plan.target.reference_model === null || plan.target.wire_tee !== null,
		'the real-Codex reference records the wire: its model is proved from its own thread/start',
	)
	.refine(
		(plan) =>
			plan.historical_reds.length === 0 ||
			(plan.build === 'baseline' && plan.journey.kind === 'compare'),
		'only a baseline comparison run may name historical reds; a head run stays strict',
	)
	.readonly();
export type CodexAcpRunPlan = z.infer<typeof codexAcpRunPlanSchema>;

/** A comparison run against a Router target reads the provider's own Usage items back. */
export function compareReadsProviderUsage(plan: CodexAcpRunPlan): boolean {
	return plan.journey.kind === 'compare' && plan.journey.provider !== 'codex';
}

/** The declared selection a Router comparison target must still hold before its first Enter. */
function compareInitialSelection(plan: CodexAcpRunPlan): DeclaredProviderSelection {
	if (plan.journey.kind !== 'compare' || plan.journey.initial_selection === null)
		throw new Error('a Router comparison target needs its initial selection');
	return plan.journey.initial_selection;
}

/** The bound for the usage readback: every reply deadline of the script plus a fixed margin. */
function journeyDeadlineMs(plan: CodexAcpRunPlan): number {
	if (plan.journey.kind !== 'compare') return 60_000;
	return plan.journey.reply_deadline_ms * 2 + plan.journey.model.outcome_deadline_ms + 120_000;
}

/** What the evaluation needs to know about this run, written next to its tee recording. */
export function compareRunRecord(plan: CodexAcpRunPlan): CompareRunRecord {
	const { journey, target } = plan;
	if (journey.kind !== 'compare' || target.wire_tee === null)
		throw new Error('a comparison run record needs a compare journey and a wire tee');
	return {
		run_kind: journey.provider === 'codex' ? 'codex-reference' : 'router-candidate',
		wire_recording: target.wire_tee.recording,
		scripted_settings: journey.scripted_settings,
		// Approvals are not driven by this journey; none are observed.
		agent_approval_outcomes: null,
	};
}

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
		serviceId: plan.target.expected_service_id,
		serviceEpoch: plan.target.expected_service_epoch,
		collaborationCli: route.collaboration_cli,
		maxPrefixBytes: route.max_prefix_bytes,
		maxSegmentBytes: route.max_segment_bytes,
	};
}

export function codexJourneyFor(plan: CodexAcpRunPlan): CodexJourney {
	const journey = plan.journey;
	if (journey.provider !== PROVIDER_FOR_ENDPOINT[plan.target.endpoint_id])
		throw new Error('journey provider differs from the target endpoint');
	if (journey.kind === 'compare') return compareJourney(journey);
	if (journey.kind === 'windows') return windowsJourneys(journey).windowA;
	if (journey.kind === 'resume') return resumeJourney(journey);
	if (journey.kind === 'refused') return refusedJourney(journey);
	if (journey.kind === 'tier') return tierJourney(journey);
	if (journey.kind === 'approval') return approvalJourney(journey);
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
	// The success cell must fit one grid row at this viewport, or its whole line cannot be read.
	if (
		journey.saved_default !== null &&
		`${CODEX_0_165_TEXT.infoPrefix}${codexSavedDefaultMessage(journey.saved_default)}`.length >
			plan.target.viewport.columns
	)
		throw new Error('the saved-default success cell would wrap at this viewport');
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
		savedDefault: journey.saved_default,
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
		['quota_storyboard::compiled_codex_acp_grid_observation', '--exact', '--list', '--ignored'],
		{ encoding: 'utf8' },
	);
	if (!listing.split('\n').includes('quota_storyboard::compiled_codex_acp_grid_observation: test'))
		throw new Error('declared Codex ACP test entry absent');
	if (realpathSync(process.execPath) !== plan.node.path || process.version !== 'v26.13.0')
		throw new Error('the run must use the declared Node v26.13.0');
	await qualifyTerminal();
	qualifyCodexAcpTarget(plan.target);
	if (plan.journey.kind === 'prompt' || compareReadsProviderUsage(plan))
		cliMcpEndpoint(plan.target);
	requireApprovalScratchBinding(plan);
	const binding = claudeRouteBinding(plan);
	if (binding !== null) {
		const observerPlan = plan.target.observer_plan;
		if (observerPlan === null) throw new Error('a route proof needs the target observer plan');
		requireClaudeRouteBinding(binding);
		verifyArtifact(binding.collaborationCli);
		const observer = z
			.looseObject({
				host_pid: z.number().int(),
				build_manifest: absolutePathSchema,
				collaboration_cli: z.string(),
			})
			.parse(JSON.parse(readFileSync(observerPlan, 'utf8')));
		if (observer.host_pid !== binding.hostPid)
			throw new Error('the route proof names another Host than its observer plan');
		// The inventory reads use the normal build's own collaboration CLI, the one its observer runs.
		const built = normalBuildManifestSchema.parse(
			JSON.parse(readFileSync(observer.build_manifest, 'utf8')),
		).binaries['agent-collaboration'];
		if (
			observer.collaboration_cli !== binding.collaborationCli.path ||
			built?.path !== binding.collaborationCli.path ||
			built.sha256 !== binding.collaborationCli.sha256
		)
			throw new Error('the route proof binds another collaboration CLI than the normal build');
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
	driver: Pick<PtyGridDriver, 'frameGeneration' | 'send' | 'waitForGrid'>,
	step: CodexJourney['steps'][number],
	keys: KeyProtocol,
	/** Stops the wait at once with its named reason (the reference's turn errors, Correction 33). */
	abortWhen: StepAbortCheck | null = null,
	/** The wire tee's running-turn reading, for a `running_turn` checkpoint (Correction 65). */
	turnRunning: (() => boolean) | null = null,
): Promise<TerminalGrid> {
	// A step that sends keys needs a frame drawn after them. A step that sends nothing observes: the
	// driver applies output as it arrives, so the frame it needs may already be on screen (D0 r2:
	// the footer redrew while the previous checkpoint was being recorded). Its checkpoint is unchanged.
	const after = step.actions.length === 0 ? -1 : driver.frameGeneration();
	await step.actions.reduce<Promise<void>>(async (sent, action) => {
		await sent;
		await driver.send(encodeCodexAction(action, keys));
	}, Promise.resolve());
	const { checkpoint } = step;
	return driver.waitForGrid(
		checkpoint.scene,
		(candidate) => {
			const abort = abortWhen?.(candidate, checkpoint) ?? null;
			if (abort !== null) throw new Error(abort);
			const signals =
				checkpoint.running_turn === true && turnRunning !== null
					? { turnRunning: turnRunning() }
					: {};
			return gridShowsCheckpoint(gridLines(candidate), checkpoint, signals);
		},
		after,
		checkpoint.deadline_ms,
	);
}

/**
 * Moved-journey hooks: whether the first typed step needs the launch composer proved first, and
 * work to run right after a named scene is recorded (the approval journey's agent-side reads).
 */
export interface DriveExtras {
	readonly launchComposer: boolean;
	readonly afterScene: ReadonlyMap<string, () => Promise<void>>;
	/** A baseline run's named reds: a red step is recorded and the remaining steps still run. */
	readonly historicalReds?: HistoricalRedContinuation;
	/**
	 * Runs once after the launch frame is proved and before any journey key: the reference's model
	 * proof. It is handed the driver's bounded frame wait so it can wait for its own readiness.
	 */
	/** A readiness the launch frame must also reach (the reference's own thread/start reply). */
	readonly launchReadiness?: { readonly ready: () => boolean; readonly pending: () => string };
	/** The reference's turn-error check: a step stops at once on it (Correction 33). */
	readonly abortWhen?: StepAbortCheck;
	readonly turnRunning?: () => boolean;
	readonly beforeFirstInput?: (
		waitForFrames: (matches: () => boolean, boundMs: number) => Promise<void>,
	) => Promise<void>;
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
	driver: Pick<PtyGridDriver, 'frameGeneration' | 'send' | 'waitForGrid'>,
	journey: CodexJourney,
	keys: KeyProtocol,
	record: (step: CodexJourney['steps'][number], grid: TerminalGrid) => Promise<void>,
	routeBaseline: RouteBaselineHook | null,
	promptReadback: PromptReadbackHooks | null = null,
	compareBeforeFirstSubmit: (() => Promise<void>) | null = null,
	launchCwd: string | null = null,
	extras: DriveExtras | null = null,
	startupBoundMs: number | null = null,
): Promise<string[]> {
	const observed: string[] = [];
	if (compareBeforeFirstSubmit !== null) {
		const submit = journey.steps.find((step) => step.checkpoint.scene === 'send-replied');
		if (JSON.stringify(submit?.actions) !== JSON.stringify([{ kind: 'key', key: 'Enter' }]))
			throw new Error('the compare model guard requires the send submission Enter');
	}
	if (promptReadback !== null) {
		const [compose, submit] = journey.steps;
		if (
			compose?.checkpoint.scene !== 'prompt-composed' ||
			submit?.checkpoint.scene !== 'prompt-replied' ||
			JSON.stringify(submit.actions) !== JSON.stringify([{ kind: 'key', key: 'Enter' }])
		)
			throw new Error('CLI MCP guard requires the ordinary prompt then submission Enter');
		await promptReadback.beforeInput();
	}
	const firstScene = journey.steps[0]?.checkpoint.scene;
	if (
		firstScene === 'prompt-composed' ||
		firstScene === 'send-composed' ||
		firstScene === 'skill-picker-open-1' ||
		extras?.launchComposer === true
	) {
		if (launchCwd === null) throw new Error('a typed journey needs its declared launch cwd');
		await requireLaunchComposer({
			driver,
			keys,
			record,
			launchCwd,
			...(startupBoundMs === null ? {} : { boundMs: startupBoundMs }),
			...(extras?.launchReadiness === undefined ? {} : { launchReadiness: extras.launchReadiness }),
		});
	}
	await extras?.beforeFirstInput?.(async (matches, boundMs) => {
		await driver.waitForGrid('reference-thread-start', () => matches(), -1, boundMs);
	});
	await journey.steps.reduce<Promise<void>>(async (previous, step, index) => {
		await previous;
		if (promptReadback !== null && step.checkpoint.scene === 'prompt-replied')
			await promptReadback.beforeSubmit();
		if (compareBeforeFirstSubmit !== null && step.checkpoint.scene === 'send-replied')
			await compareBeforeFirstSubmit();
		const reds = extras?.historicalReds;
		if (reds?.active === true) {
			observed.push(
				await reds.driveStep({
					driver,
					step,
					driveStep: (stepDriver, declared) => driveCodexStep(stepDriver, declared, keys),
					record,
				}),
			);
			await extras?.afterScene.get(step.checkpoint.scene)?.();
			return;
		}
		const grid = await driveCodexStep(
			driver,
			step,
			keys,
			extras?.abortWhen ?? null,
			extras?.turnRunning ?? null,
		);
		await record(step, grid);
		observed.push(step.checkpoint.scene);
		await extras?.afterScene.get(step.checkpoint.scene)?.();
		if (promptReadback !== null && step.checkpoint.scene === 'prompt-replied')
			await promptReadback.afterReply(gridLines(grid));
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

/**
 * Window B: a second concurrent driver and CLI on the same thread. It attaches right after
 * `attachScene` is recorded in window A, and after `observeScene` it must draw its own scene,
 * quit and settle its frames.
 */
function watcherHooks(
	plan: CodexAcpRunPlan,
	watcher: CodexJourney,
	scenes: { readonly attach: string; readonly observe: string },
	holder: { current: WatchingWindow | null },
	wireWakers: Set<() => void>,
): [string, () => Promise<void>][] {
	return [
		[
			scenes.attach,
			async (): Promise<void> => {
				holder.current = await attachWatchingWindow(plan, watcher, {
					driveCodexJourney,
					exitCodex,
					codexChild,
					registerWireWake: (wake): void => {
						wireWakers.add(wake);
					},
				});
			},
		],
		[
			scenes.observe,
			async (): Promise<void> => {
				const window = holder.current;
				if (window === null) throw new Error('window B never attached');
				await window.observeAndQuit();
			},
		],
	];
}

/**
 * The watcher a plan declares, with the scenes of window A it hangs on; none for one-window plans.
 * B resumes A's thread on the real-Codex reference, and that thread has a rollout only after A's
 * first user message (review F2). The windows journey therefore attaches B after its turn 1 on both
 * halves. On the reference, an approval's B attaches once the approval is shown, after A's submit.
 */
export function watcherFor(
	plan: CodexAcpRunPlan,
): { watcher: CodexJourney; attach: string; observe: string } | null {
	const { journey } = plan;
	const reference = plan.target.reference_model !== null;
	if (journey.kind === 'windows')
		return {
			watcher: windowsJourneys(journey).windowB,
			// Both halves: B attaches after turn 1 materialized the thread (Correction 32).
			attach: WINDOW_B_ATTACH_SCENE,
			observe: 'window-a-replied',
		};
	if (journey.kind === 'approval')
		return {
			watcher: approvalWatcherJourney(journey),
			attach: reference ? 'approval-prompt-shown' : 'approval-trigger-composed',
			observe: 'approval-turn-ended',
		};
	return null;
}

/** Hooks on the same scene run in the order given. */
function mergeSceneHooks(
	hooks: readonly (readonly [string, () => Promise<void>])[],
): Map<string, () => Promise<void>> {
	const merged = new Map<string, () => Promise<void>>();
	for (const [scene, hook] of hooks) {
		const earlier = merged.get(scene);
		merged.set(
			scene,
			earlier === undefined
				? hook
				: async (): Promise<void> => {
						await earlier();
						await hook();
					},
		);
	}
	return merged;
}

export async function runCodexAcp(declaredPlan: CodexAcpRunPlan): Promise<void> {
	const plan = codexAcpRunPlanSchema.parse(declaredPlan);
	const journey = await checkCodexAcp(plan);
	requireRedsInJourney(plan.historical_reds, journey);
	const historicalReds = new HistoricalRedContinuation(plan.historical_reds);
	requireFreshOutput(plan);
	mkdirSync(plan.output, { recursive: true, mode: 0o700 });
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
	// The wire tee listens before the wrapper spawns Codex, which connects to it, and closes after.
	const wireWakers = new Set<() => void>();
	const tee =
		plan.target.wire_tee === null
			? null
			: await CodexWireTee.listen({
					listenSocket: plan.target.wire_tee.listen_socket,
					upstreamSocket: codexSocketPath(plan.target),
					recordingPath: plan.target.wire_tee.recording,
					// A wait that reads the recording (the reference's launch readiness) wakes on a new
					// frame, not only on terminal output (review F9).
					onRecord: (): void => {
						for (const wake of wireWakers) wake();
					},
				});
	const usageReadback = compareReadsProviderUsage(plan)
		? compareUsageReadback({
				target: plan.target,
				evidence: plan.output,
				deadlineMs: journeyDeadlineMs(plan),
				selection: compareInitialSelection(plan),
			})
		: null;
	const approval = approvalHooksFor(
		plan,
		() => new Promise<void>((resolve) => setTimeout(resolve, 1_000)),
	);
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
	wireWakers.add(() => {
		driver.wakeWaiters();
	});
	const { columns, rows } = plan.target.viewport;
	let observed: string[] = [];
	const originals: unknown[] = [];
	let child: ProcessIdentity | null = null;
	const windowB: { current: WatchingWindow | null } = { current: null };
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
							...routerSession(plan.target),
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
		await usageReadback?.beforeJourney();
		await approval?.beforeJourney();
		const watcher = watcherFor(plan);
		const sceneHooks: [string, () => Promise<void>][] = [
			...(approval === null
				? []
				: [
						['approval-prompt-shown', approval.afterPromptShown] as [string, () => Promise<void>],
						[
							'approval-turn-ended',
							async (): Promise<void> => {
								await approval.afterTurnEnded();
							},
						] as [string, () => Promise<void>],
					]),
			...(watcher === null
				? []
				: watcherHooks(plan, watcher.watcher, watcher, windowB, wireWakers)),
		];
		observed = await driveCodexJourney(
			driver,
			journey,
			plan.keys,
			record,
			routeBaseline,
			plan.journey.kind === 'prompt'
				? cliPromptReadback({
						target: plan.target,
						declaration: plan.journey,
						evidence: plan.output,
					})
				: null,
			usageReadback?.beforeFirstSubmit ?? null,
			plan.target.cwd,
			{
				launchComposer: ['resume', 'refused', 'tier', 'approval', 'windows'].includes(
					plan.journey.kind,
				),
				afterScene: mergeSceneHooks(sceneHooks),
				historicalReds,
				...(plan.target.wire_tee === null
					? {}
					: { turnRunning: wireTurnReading(plan.target.wire_tee.recording, plan.target) }),
				...(plan.target.reference_model === null || plan.target.wire_tee === null
					? {}
					: {
							launchReadiness: referenceLaunchReadiness(plan.target.wire_tee.recording),
							abortWhen: referenceTurnFailure(plan.target.wire_tee.recording),
							beforeFirstInput: referenceModelProof({
								recordingPath: plan.target.wire_tee.recording,
								declared: plan.target.reference_model,
								output: plan.output,
								boundMs: plan.target.startup_deadline_ms,
							}),
						}),
			},
			plan.target.startup_deadline_ms,
		);
		if (historicalReds.active) {
			historicalReds.settle(plan.output);
			const expected = historicalReds.expectedScenes(journey);
			if (JSON.stringify(observed) !== JSON.stringify(expected))
				throw new Error(
					`checkpoint sequence differs: observed ${observed.join(',')}; expected ${expected.join(',')}`,
				);
		} else requireCheckpointSequence(observed, journey);
		await exitCodex(driver, plan.keys);
		await usageReadback?.afterJourney();
		// The PTY has ended: every byte of the run is retained, so every completed frame can be
		// inspected, not only the checkpoint frames.
		const liveLines = gridLines(driver.grid());
		if ((await driver.finish()) !== 0) throw new Error('Codex ACP driver failed');
		await settleWholeRunFrames({
			bytes: driver.bytes(),
			resizes: driver.resizeHistory(),
			liveLines,
			// A named red's error cell is the one error a baseline run with reds may show.
			policy: historicalReds.active ? 'scoped' : (journey.whole_run_errors ?? 'record'),
			expectedErrors: [...expectedErrorCells(journey), ...historicalReds.expectedErrorCells()],
			output: plan.output,
		});
	} finally {
		await windowB.current?.stop();
		await driver.stop();
		await tee?.close();
	}
	if (tee?.relayFailure !== undefined)
		throw new Error(`wire tee relay failed: ${tee.relayFailure.message}`);
	if (plan.journey.kind === 'compare')
		writeFileSync(
			join(plan.output, RUN_RECORD_FILE),
			JSON.stringify(compareRunRecord(plan), null, 2),
			{ mode: 0o600, flag: 'wx' },
		);
	writeMovedRunRecord(plan);
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
					'Ordinary prompt MCP reads retain Session/input/turn/outcome/reply; the native CLI operation identifier is unobservable. Other provider effects, history and settings need the same-session observer.',
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
