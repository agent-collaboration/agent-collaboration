/**
 * The running-turn signal (assignment Correction 65). Scenes used the literal `Working` row as their
 * only proof that a turn is running, but real Codex 0.166.0 hides it while a reply streams (D0 r1
 * c50, c64), so the check depended on timing. A checkpoint with `running_turn` is satisfied by any
 * of: the screen shows `Working`; the screen shows the in-turn composer hint while the composer holds
 * text (C51); or, for a target with a wire tee, the judged thread's current turn has `turn/started`
 * and no `turn/completed` on the wire at the time the frame is judged. The wire reading uses the
 * existing tee recording, the reference's launch-thread selection (C48-C59) and the turn reducer.
 */
import { existsSync, readFileSync } from 'node:fs';

import { z } from 'zod';

import { parseWireRecording } from '../../../../verification/observations/providers/acp/codex-wire-tee.ts';
import { selectLaunchThread } from '../../../../verification/observations/providers/acp/reference-launch-thread.ts';

const turnFrameSchema = z.looseObject({
	method: z.enum(['turn/started', 'turn/completed']),
	params: z.looseObject({ threadId: z.string(), turn: z.looseObject({ ['id']: z.string() }) }),
});

/**
 * Whether the judged thread's latest turn is running on the wire, as the recording stands: its last
 * turn/started has no turn/completed for the same turn id, counting only turn frames whose
 * `threadId` is the judged thread (review F17). The real-Codex reference judges its launch thread,
 * and a selection with any violation reads false; a Router target judges the Session thread it
 * resumed. The screen signals still apply either way.
 */
export function wireTurnRunning(recordingPath: string, judgedThread: JudgedThread): boolean {
	if (!existsSync(recordingPath)) return false;
	const recorded = parseWireRecording(readFileSync(recordingPath, 'utf8'));
	let thread: string | undefined;
	if (judgedThread.kind === 'reference') {
		const selection = selectLaunchThread(recorded);
		if (selection.violations.length > 0) return false;
		thread = selection.launchThreadId;
	} else thread = judgedThread.threadId;
	if (thread === undefined) return false;
	const started: string[] = [];
	const completed = new Set<string>();
	for (const frame of recorded) {
		if (frame.direction !== 'serverToCli') continue;
		const turn = turnFrameSchema.safeParse(frame.message);
		if (!turn.success || turn.data.params.threadId !== thread) continue;
		if (turn.data.method === 'turn/started') started.push(turn.data.params.turn.id);
		else completed.add(turn.data.params.turn.id);
	}
	const latest = started.at(-1);
	return latest !== undefined && !completed.has(latest);
}

/** Which thread a run's running-turn reading judges. */
export type JudgedThread =
	| { readonly kind: 'reference' }
	| { readonly kind: 'session'; readonly threadId: string };

/** The run's reading for a target with a tee: the reference's launch thread, or the resumed Session. */
export function wireTurnReading(
	recordingPath: string,
	target: { readonly reference_model: unknown; readonly native_thread_id: string | null },
): () => boolean {
	const judged: JudgedThread =
		target.reference_model !== null || target.native_thread_id === null
			? { kind: 'reference' }
			: { kind: 'session', threadId: target.native_thread_id };
	return (): boolean => wireTurnRunning(recordingPath, judged);
}
