/** Literal source ABI controls; these do not qualify a producer artifact or native run. */
import { expect, test } from 'vitest';

import { requireQuotaRefreshPublication } from './quota-refresh-publication.ts';
const result = {
	kind: 'failed' as const,
	failed: [{ accountId: 'effect-account-1', message: 'actual failure' }],
	overall: null,
};
const expected = { mount: 2, afterFrame: 7, result };
const publication = { kind: 'refresh-result-published', mount: 2, publicationFrame: 8, result };
test('a completed current-mount frame retains the entire actual typed result', () => {
	expect(requireQuotaRefreshPublication(publication, expected)).toEqual(publication);
});
test.each([
	['old mount', { ...publication, mount: 1 }],
	['cached frame', { ...publication, publicationFrame: 7 }],
	['old frame', { ...publication, publicationFrame: 6 }],
	['wrong variant', { ...publication, result: { kind: 'refreshed' } }],
	['lost reason', { ...publication, result: { ...result, failed: [] } }],
	['lost overall', { ...publication, result: { ...result, overall: 'invented' } }],
	['invented native ack', { ...publication, sequence: 9 }],
])('publication rejects %s', (_name, value) => {
	expect(() => requireQuotaRefreshPublication(value, expected)).toThrow();
});
