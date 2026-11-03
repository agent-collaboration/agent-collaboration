/** Callback/draw snapshots and serialized-output ordinals retain append-only identity. */
import { lstatSync, readFileSync } from 'node:fs';
import type { Stats } from 'node:fs';
import { basename, isAbsolute } from 'node:path';

import { z } from 'zod';

import { showcaseStateSchema, PrerequisiteGap } from './contracts.ts';
import type { ShowcaseState } from './contracts.ts';
import type { ReceiptNotifications } from './receipt-notifications.ts';
const serializationSchema = z.strictObject({
	kind: z.literal('serializedFrame'),
	sequence: z.number().int().positive().max(Number.MAX_SAFE_INTEGER),
	drawObservation: z.number().int().positive().max(Number.MAX_SAFE_INTEGER),
	inputOrdinal: z.number().int().nonnegative().max(Number.MAX_SAFE_INTEGER),
});
const recordSchema = z.discriminatedUnion('kind', [
	z.strictObject({ kind: z.literal('state'), snapshot: showcaseStateSchema }),
	serializationSchema,
]);
export type SerializedFrame = z.infer<typeof serializationSchema>;
export interface StateJournal {
	readonly states: readonly ShowcaseState[];
	readonly serializations: readonly SerializedFrame[];
}
export function serializedState(options: {
	readonly journal: StateJournal;
	readonly receivedFrames: number;
	readonly inputOrdinal: number;
	readonly minimumObservation: number;
}): ShowcaseState | null {
	const serialized = options.journal.serializations[options.receivedFrames - 1];
	if (
		serialized === undefined ||
		serialized.sequence !== options.journal.serializations.at(-1)?.sequence ||
		serialized.sequence !== options.receivedFrames ||
		serialized.inputOrdinal < options.inputOrdinal ||
		serialized.drawObservation < options.minimumObservation
	)
		return null;
	return options.journal.states[serialized.drawObservation - 1] ?? null;
}
function ownedMetadata(path: string): Stats {
	const metadata = lstatSync(path);
	if (
		!isAbsolute(path) ||
		basename(path) !== 'showcase-state.jsonl' ||
		metadata.isSymbolicLink() ||
		!metadata.isFile() ||
		metadata.uid !== process.getuid?.()
	)
		throw new PrerequisiteGap('state readback is not the owned example file');
	return metadata;
}
/** Validate each new record once; every read still checks the entire immutable byte prefix. */
export class ShowcaseJournal {
	private readonly path: string;
	private readonly identity: Stats;
	private previousBytes: Buffer = Buffer.alloc(0);
	private completeOffset = 0;
	private readonly states: ShowcaseState[] = [];
	private readonly serializations: SerializedFrame[] = [];
	private failure: Error | null = null;
	public constructor(path: string) {
		this.path = path;
		this.identity = ownedMetadata(path);
	}
	public read(final = false): StateJournal {
		if (this.failure !== null) throw this.failure;
		try {
			const metadata = ownedMetadata(this.path);
			if (metadata.dev !== this.identity.dev || metadata.ino !== this.identity.ino)
				throw new PrerequisiteGap('state file identity changed');
			const bytes = readFileSync(this.path);
			if (
				bytes.length < this.previousBytes.length ||
				!bytes.subarray(0, this.previousBytes.length).equals(this.previousBytes)
			)
				throw new PrerequisiteGap('state journal was truncated or rewritten');
			const complete = bytes.lastIndexOf(10) + 1;
			if (final && complete !== bytes.length)
				throw new PrerequisiteGap('final state record is truncated');
			const text = new TextDecoder('utf8', { fatal: true }).decode(
				bytes.subarray(this.completeOffset, complete),
			);
			if (text !== '')
				for (const line of text.slice(0, -1).split('\n')) {
					const record = recordSchema.parse(JSON.parse(line));
					if (record.kind === 'state') {
						const row = record.snapshot;
						if (row.observation !== this.states.length + 1)
							throw new PrerequisiteGap('state observations are not contiguous');
						const before = this.states.at(-1);
						if (
							before !== undefined &&
							row.inputOrdinal !== before.inputOrdinal + (row.cause === 'input' ? 1 : 0)
						)
							throw new PrerequisiteGap('state input ordinal does not match callbacks');
						this.states.push(row);
					} else {
						if (record.drawObservation <= (this.serializations.at(-1)?.drawObservation ?? 0))
							throw new PrerequisiteGap('serialized draw was repeated or reordered');
						if (record.sequence !== this.serializations.length + 1)
							throw new PrerequisiteGap('serialization ordinals are not contiguous');
						const draw = this.states[record.drawObservation - 1];
						if (draw?.cause !== 'frame' || draw.inputOrdinal !== record.inputOrdinal)
							throw new PrerequisiteGap(
								'serialized output does not reference its earlier completed draw',
							);
						this.serializations.push(record);
					}
				}
			this.completeOffset = complete;
			this.previousBytes = bytes;
			return { states: this.states, serializations: this.serializations };
		} catch (error) {
			this.failure = error instanceof Error ? error : new Error(String(error));
			throw this.failure;
		}
	}
}
export function readJournal(path: string, final = false): StateJournal {
	return new ShowcaseJournal(path).read(final);
}
export async function observeState(options: {
	readonly notifications: ReceiptNotifications;
	readonly journal: ShowcaseJournal;
	readonly matches: (state: ShowcaseState) => boolean;
	readonly inject: () => Promise<void>;
	readonly description: string;
}): Promise<ShowcaseState> {
	return options.notifications.wait({
		description: options.description,
		inject: options.inject,
		matches: () => options.journal.read().states.findLast(options.matches) ?? null,
	});
}
