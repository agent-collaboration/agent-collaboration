import { expect, test } from 'vitest';

import { serializedFrameCount } from './synchronized-frames.ts';
test('only actual paired synchronized blocks count, including partial transport tails', () => {
	const begin = '\x1b[?2026h',
		end = '\x1b[?2026l';
	expect(serializedFrameCount(Buffer.from(end))).toBe(0);
	expect(serializedFrameCount(Buffer.from(begin + '界' + end + end))).toBe(1);
	expect(serializedFrameCount(Buffer.from(begin + 'one' + end + begin + 'two'))).toBe(1);
	expect(serializedFrameCount(Buffer.from(begin + 'one' + end + begin + 'two' + end))).toBe(2);
	expect(() => serializedFrameCount(Buffer.from(begin + begin + end))).toThrow('nested');
});
