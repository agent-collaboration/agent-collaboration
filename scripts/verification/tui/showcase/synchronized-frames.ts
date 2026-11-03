/** Only a paired begin/end is a frame; terminal cleanup may emit an unpaired end. */
export function serializedFrameCount(bytes: Buffer): number {
	const text = bytes.toString('latin1'),
		begin = '\x1b[?2026h',
		end = '\x1b[?2026l';
	let active = false,
		count = 0;
	for (let index = 0; index < text.length; index += 1) {
		if (text.startsWith(begin, index)) {
			if (active) throw new Error('nested synchronized frame');
			active = true;
			index += begin.length - 1;
		} else if (text.startsWith(end, index)) {
			if (active) count += 1;
			active = false;
			index += end.length - 1;
		}
	}
	return count;
}
