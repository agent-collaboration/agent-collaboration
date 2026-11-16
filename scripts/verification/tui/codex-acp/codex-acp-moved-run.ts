/**
 * Run-side wiring for the moved O5 journeys (`/resume`, the tier pick, approval): the run record
 * each one leaves for codex-moved-evaluation.ts, the approval journey's agent-side hooks, and the
 * checks that tie an approval plan to the Session it runs on. Kept apart from codex-acp-run.ts so
 * that file stays the journey driver. Built and unit-tested only; the live runs are D3's and D4's.
 */
import { existsSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { z } from 'zod';

import {
	APPROVAL_RUN_FILE,
	SCRATCH_FILE_FILE,
} from '../../../../verification/observations/providers/acp/codex-approval-compare.ts';
import type { ApprovalRunRecord } from '../../../../verification/observations/providers/acp/codex-approval-compare.ts';
import { MOVED_RUN_FILE } from '../../../../verification/observations/providers/acp/codex-moved-evaluation.ts';
import type { MovedRunRecord } from '../../../../verification/observations/providers/acp/codex-moved-evaluation.ts';
import { approvalReadback, mcpApprovalPort } from './codex-acp-approval-readback.ts';
import {
	readScratchFile,
	requireFreshScratch,
	scratchInsideOutput,
} from './codex-acp-approval-scenes.ts';
import { readReferenceThreadStart } from './codex-acp-reference-start.ts';
import type { CodexAcpRunPlan } from './codex-acp-run.ts';
import { routerSession } from './codex-acp-target.ts';

const POLL_INTERVAL_MS = 1_000;
/** Added to the case deadline for every approval record read. */
export const APPROVAL_READ_MARGIN_MS = 30_000;

/** The record a moved journey run leaves next to its recording; null for every other journey. */
export function movedRunRecord(plan: CodexAcpRunPlan): MovedRunRecord | null {
	const { journey, target } = plan;
	if (journey.kind !== 'resume' && journey.kind !== 'tier' && journey.kind !== 'windows')
		return null;
	if (target.wire_tee === null) throw new Error('a moved journey run needs a wire tee');
	const wire_recording = target.wire_tee.recording;
	if (journey.kind === 'resume')
		return {
			kind: 'resume',
			wire_recording,
			// A Router target launches on its declared Session; the real-Codex reference on the thread its
			// own thread/start reported on the wire (assignment Correction 29).
			launch_thread_id:
				target.reference_model === null
					? routerSession(target).nativeThreadId
					: readReferenceThreadStart(wire_recording).thread_id,
			expected_thread_id: journey.expected_thread_id,
		};
	// A is the first connection through the tee (launched first), B the second (attached after A typed).
	if (journey.kind === 'windows')
		return { kind: 'windows', wire_recording, window_a: 0, window_b: 1 };
	return { kind: 'tier', wire_recording, wire_value: journey.tier.wire_value };
}

/**
 * The record an approval run leaves for codex-approval-compare.ts. The approving window is the
 * first connection through the tee and the watching window the second.
 */
export function approvalRunRecord(plan: CodexAcpRunPlan): ApprovalRunRecord | null {
	const { journey, target } = plan;
	if (journey.kind !== 'approval') return null;
	if (target.wire_tee === null) throw new Error('an approval run needs a wire tee');
	return {
		run_kind: journey.provider === 'codex' ? 'codex-reference' : 'router-candidate',
		wire_recording: target.wire_tee.recording,
		window_a: 0,
		window_b: 1,
		action_kind: journey.action_kind,
		cli_choice: journey.answer.case === 'timeout' ? null : journey.answer.cli_decision,
	};
}

export function writeMovedRunRecord(plan: CodexAcpRunPlan): void {
	const approval = approvalRunRecord(plan);
	if (approval !== null)
		writeFileSync(join(plan.output, APPROVAL_RUN_FILE), JSON.stringify(approval, null, 2), {
			mode: 0o600,
			flag: 'wx',
		});
	const record = movedRunRecord(plan);
	if (record !== null)
		writeFileSync(join(plan.output, MOVED_RUN_FILE), JSON.stringify(record, null, 2), {
			mode: 0o600,
			flag: 'wx',
		});
}

const observerCwdSchema = z.looseObject({ cwd: z.string() });

/**
 * An approval run's scratch directory is the Session's own working directory, the one the
 * observer plan declares, and it lies inside the run output; the trigger can write nowhere else.
 */
export function requireApprovalScratchBinding(plan: CodexAcpRunPlan): void {
	const { journey, target } = plan;
	if (journey.kind !== 'approval') return;
	// A native Codex thread has no observer plan: its working directory is where the CLI is launched.
	if (journey.provider === 'codex') {
		if (target.cwd !== journey.scratch_directory)
			throw new Error('the CLI launch directory is not the declared scratch directory');
	} else {
		if (target.observer_plan === null)
			throw new Error('an approval run needs the Session observer plan');
		const sessionCwd = observerCwdSchema.parse(
			JSON.parse(readFileSync(target.observer_plan, 'utf8')),
		).cwd;
		if (sessionCwd !== journey.scratch_directory)
			throw new Error('the Session cwd is not the declared scratch directory');
	}
	if (!scratchInsideOutput(journey.scratch_directory, plan.output))
		throw new Error('the scratch directory lies outside the run output');
}

/**
 * The run output must be new. An approval run is the one exception: its Session already exists
 * with the scratch directory as its cwd, so the output may hold that scratch directory and
 * nothing else.
 */
export function requireFreshOutput(plan: CodexAcpRunPlan): void {
	if (!existsSync(plan.output)) return;
	const { journey, output } = plan;
	if (journey.kind === 'approval' && scratchInsideOutput(journey.scratch_directory, output)) {
		const top = journey.scratch_directory.slice(output.length + 1).split('/')[0];
		const entries = readdirSync(output);
		if (entries.length === 1 && entries[0] === top) return;
	}
	throw new Error('fresh Codex ACP output required');
}

/** What the run drives around the CLI to read the agent side of an approval. */
export interface ApprovalRunHooks {
	readonly beforeJourney: () => Promise<void>;
	readonly afterPromptShown: () => Promise<void>;
	readonly afterTurnEnded: () => Promise<unknown>;
}

/**
 * Hooks for the approval journey's agent-side reading, or null for every other journey. A Router
 * Session reads the Router approval record and the scratch file; real Codex is its own agent, so
 * only the scratch file's state is kept (its outcome is read from its own wire).
 */
export function approvalHooksFor(
	plan: CodexAcpRunPlan,
	pause: () => Promise<void>,
): ApprovalRunHooks | null {
	const { journey, target } = plan;
	if (journey.kind !== 'approval') return null;
	if (journey.provider === 'codex')
		return {
			beforeJourney: () => {
				requireFreshScratch(journey.scratch_directory);
				return Promise.resolve();
			},
			afterPromptShown: () => Promise.resolve(),
			afterTurnEnded: () => {
				const state = readScratchFile(journey.scratch_directory, journey.file_name);
				writeFileSync(
					join(plan.output, SCRATCH_FILE_FILE),
					JSON.stringify({ file_present: state === 'present' }),
					{ mode: 0o600, flag: 'wx' },
				);
				return Promise.resolve();
			},
		};
	return approvalReadback({
		port: mcpApprovalPort({
			target,
			evidence: plan.output,
			deadlineMs: journey.reply_deadline_ms + journey.answer.deadline_ms + 120_000,
		}),
		declaration: journey,
		evidence: plan.output,
		pause,
		// The stated bound: the case's own deadline plus a fixed margin; reaching it fails the run.
		deadlineMs: journey.answer.deadline_ms + APPROVAL_READ_MARGIN_MS,
		pollIntervalMs: POLL_INTERVAL_MS,
	});
}
