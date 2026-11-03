/** Post-flush publication facts are independent of native input ACKs and later F12 frames. */
import { z } from 'zod';

import { canonicalJson } from '../../evidence/source-binding.ts';
import { quotaRefreshResultSchema } from '../scenes/effect-checks.ts';
export const quotaRefreshPublicationSchema = z.strictObject({
	kind: z.literal('refresh-result-published'),
	mount: z.number().int().positive().max(Number.MAX_SAFE_INTEGER),
	publicationFrame: z.number().int().positive().max(Number.MAX_SAFE_INTEGER),
	result: quotaRefreshResultSchema,
});
export function requireQuotaRefreshPublication(
	value: unknown,
	expected: {
		readonly mount: number;
		readonly afterFrame: number;
		readonly result: z.infer<typeof quotaRefreshResultSchema>;
	},
): z.infer<typeof quotaRefreshPublicationSchema> {
	const observed = quotaRefreshPublicationSchema.parse(value);
	if (
		observed.mount !== expected.mount ||
		observed.publicationFrame <= expected.afterFrame ||
		canonicalJson(observed.result) !== canonicalJson(expected.result)
	)
		throw new Error(
			'refresh publication is stale, belongs to another mount or differs from the actual mapped result',
		);
	return observed;
}
