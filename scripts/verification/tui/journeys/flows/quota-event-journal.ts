/** Append-only S7 fixture facts. Event bodies are decoded by their owning quota adapters. */
import { lstatSync, readFileSync, realpathSync, watch } from 'node:fs';
import type { BigIntStats, FSWatcher } from 'node:fs';
import { dirname, isAbsolute } from 'node:path';

import { z } from 'zod';

const envelopeSchema = z.strictObject({
	sequence: z.number().int().positive().max(Number.MAX_SAFE_INTEGER),
	event: z.object({ kind: z.string().min(1) }).catchall(z.unknown()),
});
type QuotaEventRecord = z.infer<typeof envelopeSchema>;
function ownedFile(path: string): BigIntStats {
	const metadata = lstatSync(path, { bigint: true });
	if (
		!isAbsolute(path) ||
		metadata.isSymbolicLink() ||
		!metadata.isFile() ||
		metadata.uid !== BigInt(process.getuid?.() ?? -1) ||
		realpathSync(path) !== path
	)
		throw new Error('quota events must be an explicit canonical owned regular file');
	return metadata;
}
export class QuotaEventJournal {
	private readonly path: string;
	private readonly identity: BigIntStats;
	private previousBytes: Buffer = Buffer.alloc(0);
	private completeOffset = 0;
	private readonly records: QuotaEventRecord[] = [];
	private failure: Error | null = null;
	private busy = false;
	constructor(path: string) {
		this.path = path;
		this.identity = ownedFile(path);
	}
	read(final = false): readonly QuotaEventRecord[] {
		if (this.failure !== null) throw this.failure;
		try {
			const metadata = ownedFile(this.path);
			if (metadata.dev !== this.identity.dev || metadata.ino !== this.identity.ino)
				throw new Error('quota event file identity changed');
			const bytes = readFileSync(this.path);
			if (
				bytes.length < this.previousBytes.length ||
				!bytes.subarray(0, this.previousBytes.length).equals(this.previousBytes)
			)
				throw new Error('quota events were truncated or rewritten');
			const complete = bytes.lastIndexOf(10) + 1;
			if (final && complete !== bytes.length)
				throw new Error('quota final event record is truncated');
			const text = new TextDecoder('utf-8', { fatal: true }).decode(
				bytes.subarray(this.completeOffset, complete),
			);
			if (text !== '')
				for (const line of text.slice(0, -1).split('\n')) {
					const record = envelopeSchema.parse(JSON.parse(line));
					if (record.sequence !== this.records.length + 1)
						throw new Error('quota event sequences are not contiguous');
					this.records.push(record);
				}
			this.completeOffset = complete;
			this.previousBytes = bytes;
			return [...this.records];
		} catch (error) {
			this.failure = error instanceof Error ? error : new Error(String(error));
			throw this.failure;
		}
	}
	/** A real writer/input completion drives the read; file notifications are only hints. */
	async waitFor<Event>(options: {
		readonly afterSequence: number;
		readonly kind: string;
		readonly decode: (value: unknown) => Event;
		readonly signal: AbortSignal;
		readonly awaitCompletion: (signal: AbortSignal) => Promise<void>;
	}): Promise<{ readonly sequence: number; readonly event: Event }> {
		if (this.busy) throw new Error('quota event observation is already active');
		options.signal.throwIfAborted();
		const records = this.read();
		if (
			!Number.isSafeInteger(options.afterSequence) ||
			options.afterSequence < 0 ||
			options.afterSequence > records.length
		)
			throw new Error('quota event boundary is outside the retained journal');
		this.busy = true;
		return new Promise((resolve, reject) => {
			const watchers: FSWatcher[] = [];
			let settled = false;
			let completed = false;
			const fail = (error: unknown): void => {
				if (settled) return;
				this.failure = error instanceof Error ? error : new Error(String(error));
				finish();
				reject(this.failure);
			};
			const abort = (): void =>
				fail(new Error('quota event observation aborted', { cause: options.signal.reason }));
			const finish = (): void => {
				settled = true;
				this.busy = false;
				for (const watcher of watchers) watcher.close();
				options.signal.removeEventListener('abort', abort);
			};
			const inspect = (): void => {
				if (settled || !completed) return;
				try {
					const record = this.read().find(
						(entry) => entry.sequence > options.afterSequence && entry.event.kind === options.kind,
					);
					if (record === undefined) return;
					const event = options.decode(record.event);
					finish();
					resolve({ sequence: record.sequence, event });
				} catch (error) {
					fail(error);
				}
			};
			try {
				// Arm before invoking the writer/input; completion forces the authoritative read.
				for (const path of [dirname(this.path), this.path]) {
					const watcher = watch(path, { persistent: false }, inspect);
					watchers.push(watcher);
					watcher.once('error', fail);
				}
				options.signal.addEventListener('abort', abort, { once: true });
				if (options.signal.aborted) {
					abort();
					return;
				}
				Promise.resolve()
					.then(() => (settled ? undefined : options.awaitCompletion(options.signal)))
					.then(() => {
						completed = true;
						inspect();
					}, fail);
			} catch (error) {
				fail(error);
			}
		});
	}
}
