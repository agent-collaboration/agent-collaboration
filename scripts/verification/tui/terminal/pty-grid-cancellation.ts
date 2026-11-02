/** One CLI invocation owns its signal handlers and the stop callbacks of its actual PTY children. */
import { AsyncLocalStorage } from 'node:async_hooks';

interface CancellationScope {
	signal: 'SIGINT' | 'SIGTERM' | null;
	readonly stops: Set<() => Promise<unknown>>;
	stopping: Promise<readonly PromiseSettledResult<unknown>[]> | null;
}
const cancellationScope = new AsyncLocalStorage<CancellationScope>();
export class PtyGridCommandAborted extends Error {
	readonly signal: 'SIGINT' | 'SIGTERM';
	constructor(signal: 'SIGINT' | 'SIGTERM') {
		super('operator-abort: ' + signal + '; no product verdict');
		this.signal = signal;
	}
}
export function throwIfPtyGridCancelled(): void {
	const scope = cancellationScope.getStore();
	if (scope?.signal) throw new PtyGridCommandAborted(scope.signal);
}
export function registerPtyGridStop(stop: () => Promise<unknown>): () => void {
	const scope = cancellationScope.getStore();
	if (scope === undefined) return () => undefined;
	throwIfPtyGridCancelled();
	scope.stops.add(stop);
	return () => {
		scope.stops.delete(stop);
	};
}
export async function withPtyGridSignalCancellation(run: () => Promise<void>): Promise<void> {
	const scope: CancellationScope = { signal: null, stops: new Set(), stopping: null };
	const stop = (signal: 'SIGINT' | 'SIGTERM'): void => {
		if (scope.signal !== null) return;
		scope.signal = signal;
		scope.stopping = Promise.allSettled([...scope.stops].map((close) => close()));
	};
	const interrupt = (): void => stop('SIGINT'),
		terminate = (): void => stop('SIGTERM');
	process.on('SIGINT', interrupt);
	process.on('SIGTERM', terminate);
	let settlement:
		| { readonly kind: 'completed' }
		| { readonly kind: 'failed'; readonly error: unknown };
	try {
		await cancellationScope.run(scope, run);
		settlement = { kind: 'completed' };
	} catch (error) {
		settlement = { kind: 'failed', error };
	} finally {
		process.off('SIGINT', interrupt);
		process.off('SIGTERM', terminate);
	}
	const settled = await scope.stopping;
	const failures: unknown[] = [];
	for (const result of settled ?? [])
		if (result.status === 'rejected') failures.push(result.reason);
	if (failures.length) throw new AggregateError(failures, 'operator-abort PTY cleanup failed');
	if (scope.signal !== null) throw new PtyGridCommandAborted(scope.signal);
	if (settlement.kind === 'failed') throw settlement.error;
}
