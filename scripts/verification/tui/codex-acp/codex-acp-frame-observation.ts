/**
 * Whole-run frame observation for a Codex ACP journey. Checkpoints sample the grid only after a
 * whole PTY read, so a read that carries several frames exposes only its last one, and an error
 * cell drawn in an earlier frame can be gone again by the next checkpoint. This replays the run's
 * complete retained PTY bytes, in order and with the driver's resize offsets, through the same
 * wterm/libghostty core, one byte at a time, and inspects every frame Codex completed.
 *
 * A completed frame is the parser's own synchronized-output mode ending (DECSET 2026, which every
 * Codex draw brackets with crossterm's SynchronizedUpdate, tui.rs:1304-1356, including its pending
 * history lines). Output outside any bracket is inspected once more at the stream end, and the
 * retained scrollback is inspected for a cell that scrolled away inside one frame. No ANSI text is
 * searched and no other renderer is used.
 */
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';

import type { GhosttyCore } from '@wterm/ghostty';

import {
	cellText,
	createTerminalCore,
	gridLines,
	snapshotGrid,
} from '../terminal/terminal-grid.ts';
import type { TerminalResize } from '../terminal/terminal-grid.ts';
import { CODEX_0_165_TEXT } from './codex-acp-scenes.ts';

// PtyGridDriver.start creates its core at this size before any recorded resize.
const DRIVER_COLUMNS = 120;
const DRIVER_ROWS = 36;

/** One error history cell, where it first appeared. */
export interface FrameErrorLine {
	// 1-based completed frame; 0 when it is seen only at the stream end or in scrollback.
	readonly frame: number;
	// The byte count after which it was seen.
	readonly byteOffset: number;
	readonly line: string;
}

export interface FrameObservation {
	readonly bytes: number;
	readonly completedFrames: number;
	// The stream ended inside a synchronized frame, so its last frame never completed.
	readonly unterminatedFrame: boolean;
	// The first appearance of each distinct error cell, in stream order.
	readonly errors: readonly FrameErrorLine[];
	// Scrollback rows the core dropped; a nonzero count means some rows were never inspected.
	readonly scrollbackDiscarded: number;
	readonly finalLines: readonly string[];
}

/** An error history cell: `■ ` at the line start (history_cell/notices.rs:331-336). */
function errorCell(line: string): string | null {
	return line.startsWith(CODEX_0_165_TEXT.errorPrefix) ? line.trimEnd() : null;
}

function scrollbackLines(core: GhosttyCore): string[] {
	return Array.from({ length: core.getScrollbackCount() }, (_, offset) =>
		Array.from({ length: core.getScrollbackLineLen(offset) }, (_unused, column) =>
			cellText(core.getScrollbackCell(offset, column)),
		).join(''),
	);
}

/** Every completed frame of these retained bytes, replayed with their resize boundaries. */
export async function observeCompletedFrames(
	bytes: Uint8Array,
	resizes: readonly TerminalResize[],
): Promise<FrameObservation> {
	let previous = 0;
	for (const resize of resizes) {
		if (resize.byteOffset < previous || resize.byteOffset > bytes.length)
			throw new Error('invalid terminal resize boundary');
		previous = resize.byteOffset;
	}
	const core = await createTerminalCore(DRIVER_COLUMNS, DRIVER_ROWS);
	try {
		const errors: FrameErrorLine[] = [];
		const seen = new Set<string>();
		const inspect = (lines: readonly string[], frame: number, byteOffset: number): void => {
			for (const line of lines) {
				const error = errorCell(line);
				if (error !== null && !seen.has(error)) {
					seen.add(error);
					errors.push({ frame, byteOffset, line: error });
				}
			}
		};
		let pendingResize = 0;
		const applyResizes = (offset: number): void => {
			for (
				let resize = resizes[pendingResize];
				resize !== undefined && resize.byteOffset === offset;
				resize = resizes[++pendingResize]
			)
				core.resize(resize.columns, resize.rows);
		};
		let completedFrames = 0;
		let synchronized = core.synchronizedOutput();
		for (let offset = 0; offset < bytes.length; offset += 1) {
			applyResizes(offset);
			core.writeRaw(bytes.subarray(offset, offset + 1));
			// Replies the replay produces (DSR, DA) are not sent anywhere; drain them.
			let reply = core.getResponse();
			while (reply !== null) reply = core.getResponse();
			const now = core.synchronizedOutput();
			if (synchronized && !now) {
				completedFrames += 1;
				inspect(gridLines(snapshotGrid(core)), completedFrames, offset + 1);
			}
			synchronized = now;
		}
		applyResizes(bytes.length);
		const finalLines = gridLines(snapshotGrid(core));
		inspect(finalLines, 0, bytes.length);
		inspect(scrollbackLines(core), 0, bytes.length);
		return {
			bytes: bytes.length,
			completedFrames,
			unterminatedFrame: synchronized,
			errors,
			scrollbackDiscarded: core.getScrollbackDiscardedCount(),
			finalLines,
		};
	} finally {
		core.dispose();
	}
}

/**
 * The replay must end on the grid the live driver ended on; otherwise its frames are not this
 * run's frames and prove nothing about it.
 */
export function requireReplayMatchesLiveGrid(
	observation: FrameObservation,
	liveLines: readonly string[],
): void {
	if (
		observation.finalLines.length !== liveLines.length ||
		observation.finalLines.some((line, index) => line !== liveLines[index])
	)
		throw new Error('frame replay does not end on the live grid');
}

/**
 * The whole run showed no error cell in any completed frame, at the stream end or in scrollback,
 * and every frame was observable. A partial observation is never a pass.
 */
export function requireNoWholeRunError(observation: FrameObservation): void {
	if (observation.completedFrames === 0)
		throw new Error('no completed frame was observed in the whole run');
	if (observation.unterminatedFrame)
		throw new Error('the whole run ended inside an unfinished frame');
	if (observation.scrollbackDiscarded > 0)
		throw new Error('scrollback rows were discarded before they could be inspected');
	const [first] = observation.errors;
	if (first !== undefined)
		throw new Error(
			`error cell in the whole run (frame ${first.frame}, after byte ${first.byteOffset}): ${first.line}`,
		);
}

/**
 * After the PTY ends: observe every completed frame, keep the evidence in the run output, require
 * the replay to end on the live grid, and, for a journey that forbids them, refuse any error cell.
 * The evidence is written before any refusal so a failed run keeps it.
 */
export async function settleWholeRunFrames(input: {
	readonly bytes: Uint8Array;
	readonly resizes: readonly TerminalResize[];
	readonly liveLines: readonly string[];
	readonly policy: 'forbid' | 'record';
	readonly output: string;
}): Promise<FrameObservation> {
	const frames = await observeCompletedFrames(input.bytes, input.resizes);
	writeFileSync(
		join(input.output, 'frame-observation.json'),
		JSON.stringify(
			{
				bytes: frames.bytes,
				completed_frames: frames.completedFrames,
				unterminated_frame: frames.unterminatedFrame,
				scrollback_discarded: frames.scrollbackDiscarded,
				errors: frames.errors,
				whole_run_errors: input.policy,
			},
			null,
			2,
		),
		{ flag: 'wx' },
	);
	requireReplayMatchesLiveGrid(frames, input.liveLines);
	if (input.policy === 'forbid') requireNoWholeRunError(frames);
	return frames;
}
