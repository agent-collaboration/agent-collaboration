/** Counted configured-source New preparation only; local/default use returned New outcomes. */
import { readFileSync } from 'node:fs';

import { z } from 'zod';

import { canonicalJson } from './source-binding.ts';
import { newSessionRequestSchema } from './source-qualified-outcome.ts';
export const preparationRequestSchema = newSessionRequestSchema.refine(
	(request) => request.machine.startsWith('configured:'),
	'counted preparation requires a configured source',
);
export type PreparationRequest = z.infer<typeof preparationRequestSchema>;
export const preparationReceiptSchema = z.strictObject({
	callCount: z.literal(1),
	request: preparationRequestSchema,
});
export const preparationProofSchema = z.discriminatedUnion('kind', [
	z.strictObject({ kind: z.literal('pending-s6') }),
	z.strictObject({
		kind: z.literal('recorded'),
		relativePath: z.string().regex(/^[a-z0-9-]+\.json$/u),
		expected: preparationRequestSchema,
	}),
]);
export function checkPreparationReadback(value: unknown, expected: PreparationRequest): void {
	const receipt = preparationReceiptSchema.parse(value);
	if (canonicalJson(receipt.request) !== canonicalJson(expected))
		throw new Error('preparation request differs from committed fields');
}
export function readPreparationReadback(path: string, expected: PreparationRequest): void {
	checkPreparationReadback(JSON.parse(readFileSync(path, 'utf8')), expected);
}
