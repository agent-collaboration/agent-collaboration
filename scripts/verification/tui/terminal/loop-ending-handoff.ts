/** Normalized apparatus facts; actual producer wire/provenance is bound by the quota adapter.
 * An entry receipt's intended exit code cannot substitute for the parent's real wait result.
 */
import { isAbsolute } from 'node:path';

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
interface HandoffFrame {
	readonly previousMount: number;
	readonly ready: (grid: TerminalGrid) => boolean;
	readonly firstFrame: {
		readonly key: string;
		readonly expected: readonly InputConsumptionExpectation[];
	};
}
export interface ChildLoginHandoff extends HandoffFrame {
	readonly kind: 'child-login';
	readonly expectedChildArguments: readonly string[];
	readonly expectedChildExitCode: 0 | 130 | 7;
	/** Causal child entry also guarantees the loop-ending action receipt was flushed. */
	readonly childEntered: (signal: AbortSignal) => Promise<unknown>;
	/** Must come from the parent's actual wait result, never child intent or UI text. */
	readonly childExited: (signal: AbortSignal) => Promise<unknown>;
	readonly remounted: (signal: AbortSignal) => Promise<unknown>;
}
export interface InvocationRemountHandoff extends HandoffFrame {
	readonly kind: 'invocation-remount';
	readonly previousInvocation: number;
	readonly storePath: string;
	readonly invocationSettled: (signal: AbortSignal) => Promise<unknown>;
	readonly invocationStarted: (signal: AbortSignal) => Promise<unknown>;
}
export type LoopEndingHandoff = ChildLoginHandoff | InvocationRemountHandoff;

const invocationSettledSchema = z.strictObject({
	invocation: z.number().int().positive(),
	initialMount: z.number().int().positive(),
	finalMount: z.number().int().positive(),
});
const invocationStartedSchema = z.strictObject({
	invocation: z.number().int().positive(),
	mount: z.number().int().positive(),
	storePath: z.string(),
});
export type HandoffLifecycle =
	| {
			readonly kind: 'child-login';
			readonly childEntry: z.infer<typeof childEntrySchema>;
			readonly childExit: z.infer<typeof childExitSchema>;
			readonly remount: z.infer<typeof remountSchema>;
	  }
	| {
			readonly kind: 'invocation-remount';
			readonly invocationSettled: z.infer<typeof invocationSettledSchema>;
			readonly invocationStarted: z.infer<typeof invocationStartedSchema>;
			readonly remount: z.infer<typeof remountSchema>;
	  };

/** The adapter parses producer wire; this boundary accepts only its explicit lifecycle facts. */
export async function observeHandoffLifecycle(
	options: LoopEndingHandoff,
	firstFact: unknown,
	signal: AbortSignal,
): Promise<HandoffLifecycle> {
	if (options.kind === 'child-login') {
		const childEntry = requireHandoffChildEntry(firstFact, options.expectedChildArguments);
		const childExit = requireHandoffChildExit(
			await observeHandoffFact(options.childExited, signal),
			options.expectedChildExitCode,
		);
		const remount = requireHandoffRemount(
			await observeHandoffFact(options.remounted, signal),
			options.previousMount,
		);
		return { kind: options.kind, childEntry, childExit, remount };
	}
	const settled = invocationSettledSchema.parse(firstFact);
	if (
		!Number.isSafeInteger(options.previousInvocation) ||
		options.previousInvocation <= 0 ||
		settled.invocation !== options.previousInvocation ||
		settled.initialMount !== options.previousMount ||
		settled.finalMount !== options.previousMount
	)
		throw new Error('handoff invocation settlement differs from the previous mounted invocation');
	const started = invocationStartedSchema.parse(
		await observeHandoffFact(options.invocationStarted, signal),
	);
	if (
		started.invocation !== settled.invocation + 1 ||
		started.mount !== settled.finalMount + 1 ||
		!isAbsolute(options.storePath) ||
		started.storePath !== options.storePath
	)
		throw new Error('handoff next invocation mount or store differs from the settled invocation');
	return {
		kind: options.kind,
		invocationSettled: settled,
		invocationStarted: started,
		remount: { mount: started.mount },
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
