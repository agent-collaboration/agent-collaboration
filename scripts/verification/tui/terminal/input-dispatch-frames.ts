/** Index producer OSC markers in the byte stream, independently of file notification order. */
import { createTerminalCore, snapshotGrid } from './terminal-grid.ts';
import type { TerminalGrid, TerminalResize } from './terminal-grid.ts';

export interface DispatchFrameBinding {
	readonly sequence: number;
	readonly frameGeneration: number;
	readonly frameEndByteOffset: number;
	readonly markerEndByteOffset: number;
}
export interface DispatchFrameObservation {
	readonly binding: DispatchFrameBinding;
	readonly grid: TerminalGrid;
	readonly bytes: Buffer;
	readonly resizes: readonly TerminalResize[];
}
export class DispatchFrameIndex {
	private pending = '';
	private pendingOffset = 0;
	private generation = 0;
	private insideFrame = false;
	private frameEnd = 0;
	private markerEnd = 0;
	private readonly bindings = new Map<number, DispatchFrameBinding>();
	private lastSequence = 0;
	append(bytes: Uint8Array): void {
		this.pending += Buffer.from(bytes).toString('latin1');
		// eslint-disable-next-line no-control-regex -- Match the actual VT wire protocol, including ESC and ST.
		const tokens = /\x1b\[\?2026([hl])|\x1b\]7770;dispatch=([^\x1b]*)\x1b\\/gu;
		let consumed = 0;
		for (const token of this.pending.matchAll(tokens)) {
			const end = this.pendingOffset + token.index + token[0].length;
			consumed = token.index + token[0].length;
			if (token[1] === 'h') this.insideFrame = true;
			else if (token[1] === 'l') {
				if (!this.insideFrame) throw new Error('frame end without synchronized start');
				this.insideFrame = false;
				this.generation += 1;
				this.frameEnd = end;
			} else {
				const sequence = Number(token[2]);
				if (!/^[1-9][0-9]*$/u.test(token[2] ?? '') || !Number.isSafeInteger(sequence))
					throw new Error('invalid dispatch marker sequence');
				const optionalHandoffMarker =
					sequence === this.lastSequence && this.isUnframedHandoff(sequence);
				if (sequence !== this.lastSequence + 1 && !optionalHandoffMarker)
					throw new Error('dispatch marker sequence is not contiguous');
				const previous = this.bindings.get(this.lastSequence);
				const adjacentCohort =
					previous !== undefined &&
					previous.frameEndByteOffset === this.frameEnd &&
					previous.markerEndByteOffset === this.pendingOffset + token.index;
				if (this.insideFrame || (this.frameEnd <= this.markerEnd && !adjacentCohort))
					throw new Error('dispatch marker lacks its own complete frame');
				this.bindings.set(sequence, {
					sequence,
					frameGeneration: this.generation,
					frameEndByteOffset: this.frameEnd,
					markerEndByteOffset: end,
				});
				this.lastSequence = sequence;
				this.markerEnd = end;
			}
		}
		this.pending = this.pending.slice(consumed);
		this.pendingOffset += consumed;
		// A legal token is shorter than 64 bytes. Retain only a possible split
		// token tail; long ordinary screen content never accumulates here.
		if (this.pending.length > 64) {
			const marker = this.pending.indexOf('\x1b]7770;dispatch=');
			if (marker >= 0 && this.pending.length - marker > 64)
				throw new Error('unterminated dispatch marker');
			const discard = this.pending.length - 64;
			this.pending = this.pending.slice(discard);
			this.pendingOffset += discard;
		}
	}
	get(sequence: number): DispatchFrameBinding | null {
		return this.bindings.get(sequence) ?? null;
	}
	/** Only the handoff mode calls this after its exact ACK and lifecycle facts.
	 * No frame is invented, and every unexplained missing/duplicate marker still fails.
	 */
	acknowledgeLoopEndingHandoff(sequence: number): void {
		if (!Number.isSafeInteger(sequence) || sequence <= 0)
			throw new Error('invalid handoff sequence');
		if (sequence === this.lastSequence && this.bindings.has(sequence)) return;
		if (sequence !== this.lastSequence + 1)
			throw new Error('handoff acknowledgement is not the next exact dispatch sequence');
		this.lastSequence = sequence;
	}
	isUnframedHandoff(sequence: number): boolean {
		return (
			Number.isSafeInteger(sequence) &&
			sequence > 0 &&
			sequence <= this.lastSequence &&
			!this.bindings.has(sequence)
		);
	}
}
/** Replay only through the marked frame, even if newer frames arrived in the same chunk. */
export async function dispatchFrameGrid(
	bytes: Buffer,
	resizes: readonly TerminalResize[],
	binding: DispatchFrameBinding,
): Promise<TerminalGrid> {
	if (binding.markerEndByteOffset > bytes.length) throw new Error('dispatch marker bytes absent');
	const core = await createTerminalCore(120, 36);
	let offset = 0;
	try {
		for (const resize of resizes) {
			if (resize.byteOffset > binding.frameEndByteOffset) break;
			core.writeRaw(bytes.subarray(offset, resize.byteOffset));
			core.resize(resize.columns, resize.rows);
			offset = resize.byteOffset;
		}
		core.writeRaw(bytes.subarray(offset, binding.frameEndByteOffset));
		if (
			core.synchronizedOutput() ||
			core.synchronizedOutputGeneration() !== binding.frameGeneration
		)
			throw new Error('dispatch frame index differs from emulator generation');
		return snapshotGrid(core);
	} finally {
		core.dispose();
	}
}
