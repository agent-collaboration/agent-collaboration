/**
 * A turn error ends the reference's waiting checkpoint at once (assignment Correction 33). In
 * D0 r1 c32, real Codex answered the first turn with a turn-level `error` (usageLimitExceeded,
 * `willRetry: false`) and showed it in an error cell, while the step kept waiting for its reply
 * until its deadline. For the real-Codex reference, a step now stops as soon as either source
 * shows such an error: the launch thread's `error` notification on the wire tee (Codex's own title
 * thread is set aside, reference-launch-thread.ts), or an error cell the step did not declare.
 * The run then ends with its normal cleanup; nothing retries.
 */
import { existsSync, readFileSync } from 'node:fs';

import { z } from 'zod';

import { parseWireRecording } from '../../../../verification/observations/providers/acp/codex-wire-tee.ts';
import { selectLaunchThread } from '../../../../verification/observations/providers/acp/reference-launch-thread.ts';
import { gridLines } from '../terminal/terminal-grid.ts';
import type { TerminalGrid } from '../terminal/terminal-grid.ts';
import { CODEX_0_165_TEXT } from './codex-acp-scenes.ts';
import type { CodexCheckpoint } from './codex-acp-scenes.ts';

/** Why a waiting step must stop now, or null to keep waiting. */
export type StepAbortCheck = (grid: TerminalGrid, checkpoint: CodexCheckpoint) => string | null;

const turnErrorSchema = z.looseObject({
	method: z.literal('error'),
	params: z.looseObject({
		error: z.looseObject({
			message: z.string().optional(),
			codexErrorInfo: z.unknown().optional(),
		}),
		willRetry: z.boolean().optional(),
		turnId: z.string().optional(),
	}),
});

/** One turn-level `error` notification on the launch thread. */
export interface WireTurnError {
	readonly seq: number;
	readonly name: string;
	readonly message: string;
	readonly turnId: string | undefined;
	readonly willRetry: boolean;
}

/** Every turn error on the launch thread, in wire order (Codex's title thread set aside). */
export function wireTurnErrors(recordingPath: string): WireTurnError[] {
	if (!existsSync(recordingPath)) return [];
	const { frames } = selectLaunchThread(parseWireRecording(readFileSync(recordingPath, 'utf8')));
	return frames.flatMap((frame) => {
		if (frame.direction !== 'serverToCli') return [];
		const parsed = turnErrorSchema.safeParse(frame.message);
		if (!parsed.success) return [];
		const { error, turnId, willRetry } = parsed.data.params;
		// Codex names most errors with a string; some carry an object (e.g. an HTTP failure's details).
		const info = error.codexErrorInfo;
		const name =
			typeof info === 'string' ? info : info === undefined ? 'unnamed' : JSON.stringify(info);
		return [
			{ seq: frame.seq, name, message: error.message ?? '', turnId, willRetry: willRetry === true },
		];
	});
}

function describe(error: WireTurnError): string {
	return `turn error ${error.name} on turn ${String(error.turnId)} (wire seq ${error.seq}): ${error.message}`;
}

/**
 * The launch thread's first turn error Codex will not retry, by name; null if none yet. An error
 * whose message is the step's declared expected error is not a failure (review F13).
 */
export function wireTurnFailure(recordingPath: string, expected?: string): string | null {
	const fatal = wireTurnErrors(recordingPath).find(
		(error) => !error.willRetry && error.message !== expected,
	);
	return fatal === undefined ? null : describe(fatal);
}

/** The text of each error cell on screen that the step did not declare. */
function undeclaredErrorTexts(grid: TerminalGrid, checkpoint: CodexCheckpoint): string[] {
	return gridLines(grid)
		.map((line) => line.trimEnd())
		.filter((line) => line.startsWith(CODEX_0_165_TEXT.errorPrefix))
		.map((line) => line.slice(CODEX_0_165_TEXT.errorPrefix.length))
		.filter((text) => text !== checkpoint.error_line);
}

/** An error cell on screen that the step did not declare, by its text; null if none. */
export function undeclaredErrorCell(
	grid: TerminalGrid,
	checkpoint: CodexCheckpoint,
): string | null {
	const text = undeclaredErrorTexts(grid, checkpoint)[0];
	return text === undefined ? null : `error cell on screen: ${CODEX_0_165_TEXT.errorPrefix}${text}`;
}

/**
 * The reference's abort check, with the step's own context:
 * - a wire turn error Codex will not retry stops the step, unless it is the step's declared
 *   expected error (review F13);
 * - an undeclared error cell stops it, unless its text is an error Codex marked for retry on the
 *   wire (`willRetry: true` exempts both sources, review F14).
 */
export function referenceTurnFailure(recordingPath: string): StepAbortCheck {
	return (grid, checkpoint): string | null => {
		const errors = wireTurnErrors(recordingPath);
		const fatal = errors.find(
			(error) => !error.willRetry && error.message !== checkpoint.error_line,
		);
		const retried = new Set(
			errors.filter((error) => error.willRetry).map((error) => error.message),
		);
		const cell = undeclaredErrorTexts(grid, checkpoint).find((text) => !retried.has(text));
		const failure =
			fatal !== undefined
				? describe(fatal)
				: cell === undefined
					? null
					: `error cell on screen: ${CODEX_0_165_TEXT.errorPrefix}${cell}`;
		return failure === null
			? null
			: `${checkpoint.scene} stopped at once: ${failure}; no retry (assignment Correction 33)`;
	};
}
