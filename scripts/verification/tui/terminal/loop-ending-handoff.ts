/** Normalized apparatus facts; actual producer wire/provenance is bound by the quota adapter.
 * An entry receipt's intended exit code cannot substitute for the parent's real wait result.
 */
import { z } from 'zod';

import { canonicalJson } from '../evidence/source-binding.ts';
import type { InputConsumptionExpectation } from '../sessions/input-consumption-schema.ts';
import type { TerminalGrid } from './terminal-grid.ts';

const childEntrySchema = z.object({
	arguments: z.array(z.string()),
	terminal: z.discriminatedUnion('kind', [
		z.strictObject({ kind: z.literal('notTerminal') }),
		z.strictObject({
			kind: z.literal('interactive'),
			canonical: z.boolean(),
			echo: z.boolean(),
			signals: z.boolean(),
			foreground: z.boolean(),
		}),
	]),
});
const childExitSchema = z.discriminatedUnion('kind', [
	z.strictObject({ kind: z.literal('exited'), code: z.number().int() }),
	z.strictObject({ kind: z.literal('signaled'), signal: z.string() }),
	z.strictObject({ kind: z.literal('wait-failed'), message: z.string() }),
]);
const remountSchema = z.object({ mount: z.number().int().positive() });
export interface LoopEndingHandoff {
	readonly previousMount: number;
	readonly expectedChildArguments: readonly string[];
	readonly expectedChildExitCode: 0 | 130 | 7;
	/** Causal child entry also guarantees the loop-ending action receipt was flushed. */
	readonly childEntered: (signal: AbortSignal) => Promise<unknown>;
	/** Must come from the parent's actual wait result, never child intent or UI text. */
	readonly childExited: (signal: AbortSignal) => Promise<unknown>;
	readonly remounted: (signal: AbortSignal) => Promise<unknown>;
	readonly ready: (grid: TerminalGrid) => boolean;
	readonly firstFrame: {
		readonly key: string;
		readonly expected: readonly InputConsumptionExpectation[];
	};
}
/** Every watcher belongs to this handoff and must release when its signal aborts. */
export function observeHandoffFact(
	inspect: (signal: AbortSignal) => Promise<unknown>,
	signal: AbortSignal,
): Promise<unknown> {
	return new Promise((resolve, reject) => {
		const abort = (): void =>
			reject(
				signal.reason instanceof Error ? signal.reason : new Error('handoff observation aborted'),
			);
		if (signal.aborted) {
			abort();
			return;
		}
		signal.addEventListener('abort', abort, { once: true });
		Promise.resolve()
			.then(() => inspect(signal))
			.then(
				(value) => {
					signal.removeEventListener('abort', abort);
					resolve(value);
				},
				(error: unknown) => {
					signal.removeEventListener('abort', abort);
					reject(error);
				},
			);
	});
}
export function requireHandoffChildEntry(
	value: unknown,
	expected: readonly string[],
): z.infer<typeof childEntrySchema> {
	const entry = childEntrySchema.parse(value);
	if (canonicalJson(entry.arguments) !== canonicalJson(expected))
		throw new Error('handoff child argv differs from captured target');
	const terminal = entry.terminal;
	if (
		terminal.kind !== 'interactive' ||
		!terminal.canonical ||
		!terminal.echo ||
		!terminal.signals ||
		!terminal.foreground
	)
		throw new Error('handoff child ran without terminal restoration');
	return entry;
}
export function requireHandoffChildExit(
	value: unknown,
	expected: 0 | 130 | 7,
): z.infer<typeof childExitSchema> {
	const exit = childExitSchema.parse(value);
	if (exit.kind !== 'exited' || exit.code !== expected)
		throw new Error('handoff parent-observed child exit differs from declared outcome');
	return exit;
}
export function requireHandoffRemount(
	value: unknown,
	previous: number,
): z.infer<typeof remountSchema> {
	const result = remountSchema.safeParse(value);
	if (
		!result.success ||
		result.data.mount <= previous ||
		!Number.isSafeInteger(previous) ||
		previous <= 0
	)
		throw new Error('handoff has no fresh quota remount');
	return result.data;
}
