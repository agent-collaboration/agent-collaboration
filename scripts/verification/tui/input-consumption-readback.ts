/** Append-only acknowledgements. Call assertComplete after the real producer has settled. */
import { lstatSync, readFileSync, watch } from 'node:fs';
import type { FSWatcher, BigIntStats } from 'node:fs';
import { basename, isAbsolute } from 'node:path';

import { z } from 'zod';

import {
	inputConsumptionExpectationSchema,
	inputDispatchReceiptSchema,
} from './input-consumption-schema.ts';
import type {
	InputConsumptionExpectation,
	InputDispatchReceipt,
} from './input-consumption-schema.ts';
import { canonicalJson } from './source-binding.ts';

interface JournalSnapshot {
	readonly receipts: readonly InputDispatchReceipt[];
	readonly partial: boolean;
}
function decodeJournal(bytes: Buffer): JournalSnapshot {
	const boundary = bytes.lastIndexOf(10) + 1;
	let text: string;
	try {
		text = new TextDecoder('utf-8', { fatal: true }).decode(bytes.subarray(0, boundary));
	} catch {
		throw new Error('input receipt contains invalid UTF-8');
	}
	const lines = text === '' ? [] : text.slice(0, -1).split('\n');
	const receipts = lines.map((line, index) => {
		let value: unknown;
		try {
			value = JSON.parse(line);
		} catch {
			throw new Error(`input receipt JSON invalid at line ${index + 1}`);
		}
		const receipt = inputDispatchReceiptSchema.safeParse(value);
		if (!receipt.success) throw new Error(`input receipt wire invalid at line ${index + 1}`);
		if (receipt.data.sequence !== index + 1)
			throw new Error(`input receipt sequence is not contiguous at line ${index + 1}`);
		return receipt.data;
	});
	return { receipts, partial: boundary !== bytes.length };
}
export function parseInputConsumptionReadback(bytes: Buffer): readonly InputDispatchReceipt[] {
	const snapshot = decodeJournal(bytes);
	if (snapshot.partial) throw new Error('input receipt has a truncated final line');
	return snapshot.receipts;
}
function compareReceipt(
	receipt: InputDispatchReceipt,
	expected: InputConsumptionExpectation,
): void {
	for (const field of ['event', 'surface', 'target', 'action'] as const)
		if (canonicalJson(receipt[field]) !== canonicalJson(expected[field]))
			throw new Error(`input acknowledgement ${field} mismatch at sequence ${receipt.sequence}`);
}
export function checkInputConsumptionReceipt(
	value: unknown,
	expected: unknown,
): InputDispatchReceipt {
	const receipt = inputDispatchReceiptSchema.parse(value);
	compareReceipt(receipt, inputConsumptionExpectationSchema.parse(expected));
	return receipt;
}
function ownedFile(path: string): BigIntStats {
	const metadata = lstatSync(path, { bigint: true });
	const owner = process.getuid?.();
	if (
		!isAbsolute(path) ||
		basename(path) !== 'input-dispatch.jsonl' ||
		!metadata.isFile() ||
		metadata.isSymbolicLink() ||
		owner === undefined ||
		metadata.uid !== BigInt(owner)
	)
		throw new Error('input receipt must be an explicit owned regular input-dispatch.jsonl file');
	return metadata;
}
export interface ObserveInputConsumption {
	readonly expected: readonly InputConsumptionExpectation[];
	readonly inject: () => Promise<void>;
	readonly timeoutMs: number;
	readonly signal?: AbortSignal;
}
/** One retained child's nominated file; source/build/child binding stays in its capture receipt. */
export class InputConsumptionReadback {
	readonly initialReceipts: readonly InputDispatchReceipt[];
	private readonly path: string;
	private readonly identity: BigIntStats;
	private previousBytes = Buffer.alloc(0);
	private cursor = 0;
	private busy = false;
	private failure: Error | null = null;
	private constructor(path: string) {
		this.path = path;
		this.identity = ownedFile(path);
		const snapshot = this.snapshot();
		if (snapshot.partial) throw new Error('incomplete input receipt before observation');
		this.initialReceipts = snapshot.receipts;
		this.cursor = snapshot.receipts.length;
	}
	static open(path: string): InputConsumptionReadback {
		return new InputConsumptionReadback(path);
	}
	get sequence(): number {
		return this.cursor;
	}
	private snapshot(): JournalSnapshot {
		const metadata = ownedFile(this.path);
		if (metadata.dev !== this.identity.dev || metadata.ino !== this.identity.ino)
			throw new Error('input receipt file identity changed');
		const bytes = readFileSync(this.path);
		if (
			bytes.length < this.previousBytes.length ||
			!bytes.subarray(0, this.previousBytes.length).equals(this.previousBytes)
		)
			throw new Error('input receipt was truncated or rewritten');
		const snapshot = decodeJournal(bytes);
		this.previousBytes = bytes;
		return snapshot;
	}
	private healthy(): void {
		if (this.failure !== null) throw this.failure;
	}
	/** Final exact-count/truncation check, only after fixture exit and flush_and_check succeed. */
	assertComplete(): readonly InputDispatchReceipt[] {
		this.healthy();
		if (this.busy) throw new Error('input observation is still active');
		try {
			const snapshot = this.snapshot();
			if (snapshot.partial) throw new Error('input receipt has a truncated final line');
			if (snapshot.receipts.length !== this.cursor)
				throw new Error('unexpected input acknowledgement beyond the declared observations');
			return snapshot.receipts;
		} catch (error) {
			this.failure = error instanceof Error ? error : new Error('input receipt read failed');
			throw this.failure;
		}
	}
	async observeInput(options: ObserveInputConsumption): Promise<readonly InputDispatchReceipt[]> {
		this.healthy();
		if (this.busy) throw new Error('input observation is already active');
		const expected = z.array(inputConsumptionExpectationSchema).min(1).parse(options.expected);
		z.number().int().positive().max(2_147_483_647).parse(options.timeoutMs);
		options.signal?.throwIfAborted();
		this.assertComplete();
		this.busy = true;
		return new Promise((resolve, reject) => {
			let watcher: FSWatcher | null = null;
			let timer: ReturnType<typeof setTimeout> | null = null;
			let finished = false;
			let injected = false;
			const abort = (): void => fail(new Error('input observation aborted'));
			const finish = (
				error: Error | null,
				receipts: readonly InputDispatchReceipt[] = [],
			): void => {
				if (finished) return;
				finished = true;
				watcher?.close();
				if (timer !== null) clearTimeout(timer);
				options.signal?.removeEventListener('abort', abort);
				this.busy = false;
				if (error !== null) {
					this.failure = error;
					reject(error);
				} else resolve(receipts);
			};
			const fail = (error: unknown): void =>
				finish(error instanceof Error ? error : new Error('input observation failed'));
			const check = (): void => {
				if (finished || !injected) return;
				try {
					const snapshot = this.snapshot();
					const next = snapshot.receipts.slice(this.cursor);
					if (next.length > expected.length)
						throw new Error('unexpected extra input acknowledgement');
					for (const [index, receipt] of next.entries()) {
						const expectation = expected[index];
						if (expectation === undefined) throw new Error('input expectation absent');
						compareReceipt(receipt, expectation);
					}
					if (next.length === expected.length && !snapshot.partial) {
						this.cursor += next.length;
						finish(null, next);
					}
				} catch (error) {
					fail(error);
				}
			};
			try {
				// Arm before injection; the immediate read also catches writes before a notification.
				watcher = watch(this.path, { persistent: false }, check);
				watcher.once('error', fail);
				timer = setTimeout(
					() => finish(new Error('input acknowledgement deadline: missing or incomplete record')),
					options.timeoutMs,
				);
				options.signal?.addEventListener('abort', abort, { once: true });
				if (options.signal?.aborted) {
					abort();
					return;
				}
				Promise.resolve()
					.then(() => (finished ? undefined : options.inject()))
					.then(() => {
						injected = true;
						check();
					}, fail);
			} catch (error) {
				fail(error);
			}
		});
	}
}
