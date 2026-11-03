/** Bind flushed S7 journal facts to the existing same-PTY lifecycle owner. */
import { z } from 'zod';

import type {
	ChildLoginHandoff,
	InvocationRemountHandoff,
} from '../../terminal/loop-ending-handoff.ts';
import {
	quotaLoginReceiptSchema,
	quotaLoginParentExitSchema,
	requireQuotaLoginParentExit,
} from './quota-effect-readback.ts';
import type { QuotaEventJournal } from './quota-event-journal.ts';
import type { QuotaFixtureControl } from './quota-fixture-control.ts';
import type { QuotaFixtureEventKind } from './quota-fixture-control.ts';
import { readQuotaGroupingSettled, readQuotaGroupingStarted } from './quota-preference-readback.ts';

interface QuotaHandoffJournal {
	readonly journal: QuotaEventJournal;
	readonly control: QuotaFixtureControl;
	readonly afterSequence: number;
}

/** A fence wakes the independent file reader; its ordinal must also exist in that file. */
export async function observeQuotaEvent<TEvent>(
	options: QuotaHandoffJournal & {
		readonly kind: QuotaFixtureEventKind;
		readonly signal: AbortSignal;
		readonly decode: (value: unknown) => TEvent;
	},
): Promise<{ readonly sequence: number; readonly event: TEvent }> {
	return options.journal.waitFor({
		afterSequence: options.afterSequence,
		kind: options.kind,
		signal: options.signal,
		decode: options.decode,
		awaitCompletion: async (signal) => {
			const completion = await options.control.awaitEvent({
				afterOrdinal: options.afterSequence,
				kind: options.kind,
				signal,
			});
			const record = options.journal.read().find((entry) => entry.sequence === completion.ordinal);
			if (record?.event.kind !== options.kind)
				throw new Error('quota event fence has no matching independently flushed journal record');
		},
	});
}

export function quotaGroupingHandoff(
	options: QuotaHandoffJournal & {
		readonly previousMount: number;
		readonly storePath: string;
		readonly ready: InvocationRemountHandoff['ready'];
		readonly firstFrame: InvocationRemountHandoff['firstFrame'];
	},
): InvocationRemountHandoff {
	let settledSequence: number | null = null;
	return {
		kind: 'invocation-remount',
		previousInvocation: 1,
		previousMount: options.previousMount,
		storePath: options.storePath,
		ready: options.ready,
		firstFrame: options.firstFrame,
		invocationSettled: async (signal) => {
			if (settledSequence !== null)
				throw new Error('quota grouping settlement was already consumed');
			const result = await observeQuotaEvent({
				...options,
				kind: 'grouping-invocation-settled',
				signal,
				decode: (value) =>
					readQuotaGroupingSettled(value, {
						invocation: 1,
						initialMount: options.previousMount,
						finalMount: options.previousMount,
						initialFacet: 'none',
						finalFacet: 'provider',
						storedValue: 'provider',
					}),
			});
			settledSequence = result.sequence;
			return result.event;
		},
		invocationStarted: async (signal) => {
			if (settledSequence === null) throw new Error('quota grouping start preceded settlement');
			const result = await observeQuotaEvent({
				...options,
				afterSequence: settledSequence,
				kind: 'grouping-invocation-started',
				signal,
				decode: (value) =>
					readQuotaGroupingStarted(value, {
						invocation: 2,
						mount: options.previousMount + 1,
						facet: 'provider',
						storePath: options.storePath,
					}),
			});
			return result.event;
		},
	};
}

const loginEnteredSchema = z.strictObject({
	kind: z.literal('login-child-entered'),
	receipt: quotaLoginReceiptSchema,
});
const parentExitedSchema = quotaLoginParentExitSchema.extend({
	kind: z.literal('login-parent-exited'),
});
const mountedSchema = z.strictObject({
	kind: z.literal('view-mounted'),
	readback: z.object({ mount: z.number().int().positive() }).catchall(z.unknown()),
});

export function quotaLoginHandoff(
	options: QuotaHandoffJournal & {
		readonly previousMount: number;
		readonly expectedChildArguments: readonly string[];
		readonly expectedChildExitCode: 0 | 130 | 7;
		readonly ready: ChildLoginHandoff['ready'];
		readonly firstFrame: ChildLoginHandoff['firstFrame'];
	},
): ChildLoginHandoff {
	let entrySequence: number | null = null;
	let exitSequence: number | null = null;
	return {
		kind: 'child-login',
		previousMount: options.previousMount,
		expectedChildArguments: options.expectedChildArguments,
		expectedChildExitCode: options.expectedChildExitCode,
		ready: options.ready,
		firstFrame: options.firstFrame,
		childEntered: async (signal) => {
			if (entrySequence !== null) throw new Error('quota login entry was already consumed');
			const result = await observeQuotaEvent({
				...options,
				kind: 'login-child-entered',
				signal,
				decode: (value) => loginEnteredSchema.parse(value).receipt,
			});
			entrySequence = result.sequence;
			// exitCode is the stand-in's plan; it cannot become a parent-wait fact.
			return { arguments: result.event.arguments, terminal: result.event.terminal };
		},
		childExited: async (signal) => {
			if (entrySequence === null) throw new Error('quota parent wait preceded child entry');
			if (exitSequence !== null) throw new Error('quota parent wait was already consumed');
			const result = await observeQuotaEvent({
				...options,
				afterSequence: entrySequence,
				kind: 'login-parent-exited',
				signal,
				decode: (value) => {
					const { kind: _kind, ...parent } = parentExitedSchema.parse(value);
					return requireQuotaLoginParentExit(parent, options.expectedChildExitCode);
				},
			});
			exitSequence = result.sequence;
			return result.event;
		},
		remounted: async (signal) => {
			if (exitSequence === null) throw new Error('quota remount preceded actual parent wait');
			const result = await observeQuotaEvent({
				...options,
				afterSequence: exitSequence,
				kind: 'view-mounted',
				signal,
				decode: (value) => ({ mount: mountedSchema.parse(value).readback.mount }),
			});
			return result.event;
		},
	};
}
