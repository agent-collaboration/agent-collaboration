/** Discover receipt creation, then retain file-write subscriptions for open writers. */
import { existsSync, lstatSync, watch } from 'node:fs';
import type { FSWatcher } from 'node:fs';
import { isAbsolute, join } from 'node:path';

import { PrerequisiteGap } from './contracts.ts';
export class ReceiptNotifications {
	private readonly watcher: FSWatcher;
	private readonly directory: string;
	private readonly files = new Map<string, FSWatcher>();
	private readonly listeners = new Set<() => void>();
	private failure: Error | null = null;
	private readonly observations: { readonly kind: string; readonly detail: string }[] = [];
	public constructor(directory: string) {
		this.directory = directory;
		const metadata = lstatSync(directory);
		if (
			!isAbsolute(directory) ||
			!metadata.isDirectory() ||
			metadata.isSymbolicLink() ||
			metadata.uid !== process.getuid?.()
		)
			throw new PrerequisiteGap('notification directory must be owned and direct');
		this.watcher = watch(directory, { persistent: false }, (_kind, name) => {
			const file = name?.toString();
			this.observations.push({ kind: 'notification', detail: String(file) });
			if (file !== 'input-dispatch.jsonl' && file !== 'showcase-state.jsonl') return;
			this.subscribe(file);
			this.notify();
		});
		this.watcher.once('error', (error) => {
			this.failure = error;
			this.notify();
		});
	}
	private subscribe(file: string): void {
		if (this.files.has(file)) return;
		const path = join(this.directory, file);
		const entry = lstatSync(path);
		if (!entry.isFile() || entry.isSymbolicLink() || entry.uid !== process.getuid?.()) {
			this.failure = new PrerequisiteGap('created receipt is not an owned regular file');
			return;
		}
		const subscription = watch(path, { persistent: false }, () => {
			this.observations.push({ kind: 'file-write', detail: file });
			this.notify();
		});
		subscription.once('error', (error) => {
			this.failure = error;
			this.notify();
		});
		this.files.set(file, subscription);
		this.observations.push({ kind: 'subscribed', detail: file });
	}
	private notify(): void {
		for (const listener of this.listeners) listener();
	}
	public async ready(): Promise<void> {
		// The initial received frame proves the writer has started. macOS can
		// coalesce its earlier directory creation event; attach to both actual
		// files directly instead of demanding that optional discovery event.
		for (const file of ['input-dispatch.jsonl', 'showcase-state.jsonl'])
			if (existsSync(join(this.directory, file))) this.subscribe(file);
		await this.wait({
			description: 'retained subscriptions to both owned receipt files',
			matches: () => (this.files.size === 2 ? true : null),
			inject: async () => {},
		});
	}
	public async wait<TResult>(options: {
		readonly description: string;
		readonly matches: () => TResult | null;
		readonly inject: () => Promise<void>;
	}): Promise<TResult> {
		this.observations.push({ kind: 'wait', detail: options.description });
		return new Promise((resolve, reject) => {
			let injected = false,
				settled = false;
			const done = (error: Error | null, result?: TResult): void => {
				if (settled) return;
				settled = true;
				clearTimeout(timer);
				this.listeners.delete(check);
				if (error !== null) reject(error);
				else if (result !== undefined) resolve(result);
			};
			const check = (): void => {
				if (!injected || settled) return;
				try {
					if (this.failure !== null) throw this.failure;
					if (this.files.size !== 2) return;
					const result = options.matches();
					this.observations.push({
						kind: result === null ? 'pending' : 'matched',
						detail: options.description,
					});
					if (result !== null) done(null, result);
				} catch (error) {
					done(error instanceof Error ? error : new Error(String(error)));
				}
			};
			const timer = setTimeout(
				() => done(new PrerequisiteGap('receipt prerequisite unavailable: ' + options.description)),
				8000,
			);
			this.listeners.add(check);
			Promise.resolve()
				.then(options.inject)
				.then(
					() => {
						injected = true;
						check();
					},
					(error: unknown) => done(error instanceof Error ? error : new Error(String(error))),
				);
		});
	}
	public history(): readonly { readonly kind: string; readonly detail: string }[] {
		return this.observations;
	}
	public close(): void {
		this.watcher.close();
		for (const subscription of this.files.values()) subscription.close();
		this.failure = new PrerequisiteGap('owned receipt observer closed');
		this.notify();
	}
}
