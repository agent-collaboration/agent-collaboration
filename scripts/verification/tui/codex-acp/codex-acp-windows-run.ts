/**
 * Window B of the O5 two-window journey: a second concurrent PtyGridDriver on the same target,
 * with its own output directory (`window-b/` under the run output) and its own CLI connection
 * through the same wire tee. The first driver is window A; B attaches after A's first turn has
 * replied (it materializes the thread, Correction 32), so the tee's connection 0 is A and connection
 * 1 is B. Built and unit-tested only (the evaluation and the plan); never launched here, because it starts a
 * second Codex CLI. D3 runs it live.
 *
 * The run's own drivers (`driveCodexJourney`, `exitCodex`, `codexChild`) are passed in, which
 * keeps this module from importing codex-acp-run.ts back.
 */
import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import type { ProcessIdentity } from '../evidence/evidence-schema.ts';
import { sameIdentity } from '../processes/process-identity.ts';
import { PtyGridDriver } from '../terminal/pty-grid-driver.ts';
import { gridLines } from '../terminal/terminal-grid.ts';
import type { TerminalGrid } from '../terminal/terminal-grid.ts';
import { renderOriginal } from '../terminal/terminal-original.ts';
import { requireLaunchComposer } from './codex-acp-composer-ready.ts';
import { settleWholeRunFrames } from './codex-acp-frame-observation.ts';
import {
	readReferenceThreadStart,
	referenceAttachReadiness,
	requireReferenceAttach,
} from './codex-acp-reference-start.ts';
import type { CodexAcpRunPlan } from './codex-acp-run.ts';
import type { CodexJourney } from './codex-acp-scenes.ts';
import type { CodexAcpTarget } from './codex-acp-target.ts';

export const WINDOW_B_DIRECTORY = 'window-b';
/** Window B of the reference: what its thread/resume reply reported (model proof). */
export const REFERENCE_THREAD_ATTACH_FILE = 'reference-thread-attach.json';

/** The run's own journey drivers, passed in by codex-acp-run.ts. */
export interface WindowDrivers {
	readonly driveCodexJourney: (
		driver: PtyGridDriver,
		journey: CodexJourney,
		keys: CodexAcpRunPlan['keys'],
		record: (step: CodexJourney['steps'][number], grid: TerminalGrid) => Promise<void>,
		routeBaseline: null,
	) => Promise<string[]>;
	readonly exitCodex: (driver: PtyGridDriver, keys: CodexAcpRunPlan['keys']) => Promise<void>;
	readonly codexChild: (output: string, target: CodexAcpTarget) => ProcessIdentity;
	/** Registers window B's driver to wake on each new wire frame (review F9). */
	readonly registerWireWake: (wake: () => void) => void;
}

/**
 * The thread window B of the real-Codex reference resumes: the one window A's own thread/start
 * opened, on A's connection (review F2). Null for a Router target, whose B resumes its Session.
 */
function referenceAttachment(
	plan: CodexAcpRunPlan,
): { readonly recording: string; readonly threadId: string; readonly windowA: number } | null {
	if (plan.target.reference_model === null || plan.target.wire_tee === null) return null;
	const recording = plan.target.wire_tee.recording;
	const started = readReferenceThreadStart(recording);
	return { recording, threadId: started.thread_id, windowA: started.connection };
}

export interface WatchingWindow {
	/** B's own scene: A's reply drawn on B's screen; then B quits normally and its frames settle. */
	readonly observeAndQuit: () => Promise<void>;
	readonly stop: () => Promise<void>;
}

/** Starts window B and waits for its launch composer; call it while A's prompt is unsubmitted. */
export async function attachWatchingWindow(
	plan: CodexAcpRunPlan,
	journey: CodexJourney,
	drivers: WindowDrivers,
): Promise<WatchingWindow> {
	const output = join(plan.output, WINDOW_B_DIRECTORY);
	mkdirSync(output, { mode: 0o700 });
	const attachment = referenceAttachment(plan);
	const target =
		attachment === null ? plan.target : { ...plan.target, attach_thread_id: attachment.threadId };
	const config = join(output, 'codex-target.json');
	writeFileSync(
		config,
		JSON.stringify({
			surface: 'codex-acp',
			binary: plan.target.codex.path,
			binary_sha256: plan.target.codex.sha256,
			target: JSON.stringify(target),
			output,
		}),
		{ mode: 0o600, flag: 'wx' },
	);
	const driver = await PtyGridDriver.start({
		repository: plan.repository,
		driver: plan.driver.path,
		surface: 'codex-acp',
		keys: plan.keys,
		evidence: output,
		publicEnvironment: {
			QUOTA_STORYBOARD_NODE: plan.node.path,
			QUOTA_STORYBOARD_NODE_SHA256: plan.node.sha256,
			TUI_GRID_CODEX_OUTPUT: output,
			TUI_GRID_CODEX_CONFIG: config,
			TUI_GRID_CODEX_CONFIG_SHA256: createHash('sha256').update(readFileSync(config)).digest('hex'),
		},
	});
	drivers.registerWireWake(() => {
		driver.wakeWaiters();
	});
	const { columns, rows } = plan.target.viewport;
	let child: ProcessIdentity | null = null;
	const record = async (step: CodexJourney['steps'][number], grid: TerminalGrid): Promise<void> => {
		const current = drivers.codexChild(output, plan.target);
		if (child !== null && !sameIdentity(current, child))
			throw new Error('Codex PTY child of window B was replaced during the journey');
		child = current;
		writeFileSync(join(output, `${step.checkpoint.scene}.grid.json`), JSON.stringify(grid));
		await renderOriginal({
			chrome: plan.chrome.path,
			output,
			bytes: driver.bytes(),
			grid,
			resizes: driver.resizeHistory(),
			columns,
			rows,
			checkpoint: step.checkpoint.scene,
			sourceHead: plan.source_head,
			binarySha256: plan.target.codex.sha256,
		});
	};
	if (columns !== 120 || rows !== 36) await driver.resize(columns, rows);
	await requireLaunchComposer({
		driver,
		keys: plan.keys,
		record,
		launchCwd: plan.target.cwd,
		boundMs: plan.target.startup_deadline_ms,
		...(attachment === null
			? {}
			: {
					launchReadiness: referenceAttachReadiness(
						attachment.recording,
						attachment.threadId,
						attachment.windowA,
					),
				}),
	});
	if (attachment !== null && plan.target.reference_model !== null) {
		// Window B's own model proof, from its thread/resume reply, before it sends any key.
		const attached = requireReferenceAttach(
			attachment.recording,
			attachment.threadId,
			attachment.windowA,
			plan.target.reference_model,
		);
		writeFileSync(join(output, REFERENCE_THREAD_ATTACH_FILE), JSON.stringify(attached, null, 2), {
			mode: 0o600,
			flag: 'wx',
		});
	}
	return {
		observeAndQuit: async (): Promise<void> => {
			await drivers.driveCodexJourney(driver, journey, plan.keys, record, null);
			await drivers.exitCodex(driver, plan.keys);
			const liveLines = gridLines(driver.grid());
			if ((await driver.finish()) !== 0) throw new Error('Codex ACP driver of window B failed');
			await settleWholeRunFrames({
				bytes: driver.bytes(),
				resizes: driver.resizeHistory(),
				liveLines,
				policy: journey.whole_run_errors ?? 'record',
				output,
			});
		},
		stop: async (): Promise<void> => {
			await driver.stop();
		},
	};
}
