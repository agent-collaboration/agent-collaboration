/** S6's recording fixture is required before this can prove Start dispatch. */
import { readFileSync } from 'node:fs';

import { z } from 'zod';

import { canonicalJson } from './evidence/source-binding.ts';
export const preparationRequestSchema = z.strictObject({
	client: z.string().min(1),
	machine: z.string().min(1),
	directory: z.string().min(1),
	runMode: z.enum(['foreground', 'background']),
});
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
