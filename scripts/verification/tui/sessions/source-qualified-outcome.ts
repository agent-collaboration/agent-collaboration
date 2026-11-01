/** Adopted U4/U5 fixture wire; this reader is not a producer or launch effect proof. */
import { lstatSync, readFileSync } from 'node:fs';
import { basename, isAbsolute } from 'node:path';

import { z } from 'zod';

import { canonicalJson } from '../evidence/source-binding.ts';
const nonemptySchema = z.string().refine((value) => value.trim().length > 0);
const sessionReferenceSchema = z.strictObject({
	endpoint: z.strictObject({
		serviceId: z.string().regex(/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/u),
		endpointId: nonemptySchema,
	}),
	sessionId: nonemptySchema,
});
export const pickerIdentityReadbackSchema = z.discriminatedUnion('kind', [
	z.strictObject({ kind: z.literal('localCodex'), sessionId: nonemptySchema }),
	z.strictObject({ kind: z.literal('hostedCodex'), target: sessionReferenceSchema }),
	z.strictObject({ kind: z.literal('hostedProvider'), target: sessionReferenceSchema }),
	z.strictObject({ kind: z.literal('nativeClaude'), target: sessionReferenceSchema }),
]);
export const newSessionRequestSchema = z.strictObject({
	client: z.enum(['codex', 'claudeCode']),
	machine: z.union([
		z.literal('defaultHosted'),
		z.literal('localCodex'),
		z
			.string()
			.regex(/^configured:.+$/u)
			.refine((value) => value.slice('configured:'.length).trim().length > 0),
	]),
	directory: nonemptySchema,
	runMode: z.enum(['foreground', 'backgroundThenAttach']),
});
export const pickerOutcomeReadbackSchema = z.discriminatedUnion('kind', [
	z.strictObject({ kind: z.literal('cancelled') }),
	z.strictObject({ kind: z.literal('resume'), identity: pickerIdentityReadbackSchema }),
	z.strictObject({ kind: z.literal('fork'), identity: pickerIdentityReadbackSchema }),
	z.strictObject({ kind: z.literal('new'), request: newSessionRequestSchema }),
]);
export type PickerOutcomeReadback = z.infer<typeof pickerOutcomeReadbackSchema>;
export type NewSessionRequest = z.infer<typeof newSessionRequestSchema>;
export function parsePickerOutcomeReadback(bytes: Buffer): PickerOutcomeReadback {
	let value: unknown;
	try {
		value = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(bytes));
	} catch {
		throw new Error('picker outcome must contain exactly one complete JSON outcome');
	}
	return pickerOutcomeReadbackSchema.parse(value);
}
export function checkSourceQualifiedOutcome(
	value: unknown,
	expected: unknown,
): PickerOutcomeReadback {
	const outcome = pickerOutcomeReadbackSchema.parse(value);
	const declared = pickerOutcomeReadbackSchema.parse(expected);
	if (canonicalJson(outcome) !== canonicalJson(declared))
		throw new Error(
			'picker outcome differs from the captured request or source-qualified identity',
		);
	return outcome;
}
export function checkLocalNewOutcome(
	value: unknown,
	expected: NewSessionRequest,
): PickerOutcomeReadback {
	const request = newSessionRequestSchema.parse(expected);
	if (request.machine.startsWith('configured:'))
		throw new Error('configured-source New requires the counted preparation stand-in');
	return checkSourceQualifiedOutcome(value, { kind: 'new', request });
}
export function readSourceQualifiedOutcome(path: string, expected: unknown): PickerOutcomeReadback {
	const metadata = lstatSync(path);
	if (
		!isAbsolute(path) ||
		basename(path) !== 'picker-outcome.json' ||
		!metadata.isFile() ||
		metadata.isSymbolicLink() ||
		metadata.uid !== process.getuid?.()
	)
		throw new Error('picker outcome must be the owned fixture picker-outcome.json file');
	return checkSourceQualifiedOutcome(parsePickerOutcomeReadback(readFileSync(path)), expected);
}
