import { mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { afterEach, expect, test } from 'vitest';

import { MOVED_RUN_FILE } from '../../../../verification/observations/providers/acp/codex-moved-evaluation.ts';
import {
	LAUNCH_THREAD,
	OTHER_THREAD,
	resumeRun,
	tierRun,
} from '../../../../verification/observations/providers/acp/codex-moved-fixtures.ts';
import type { WireFrame } from '../../../../verification/observations/providers/acp/turn-skeleton.ts';
import { MOVED_VERDICT_FILE, runMovedCommand } from './codex-acp-moved.ts';

const roots: string[] = [];
afterEach(() => {
	for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
});

function runDirectory(record: unknown, frames: readonly WireFrame[]): string {
	const directory = realpathSync(mkdtempSync(join(tmpdir(), 'moved-cmd-')));
	roots.push(directory);
	mkdirSync(directory, { recursive: true });
	writeFileSync(
		join(directory, 'wire.jsonl'),
		frames.map((frame) => JSON.stringify(frame)).join('\n'),
	);
	writeFileSync(join(directory, MOVED_RUN_FILE), JSON.stringify(record));
	return directory;
}
const RESUME_RECORD = {
	kind: 'resume',
	wire_recording: 'wire.jsonl',
	launch_thread_id: LAUNCH_THREAD,
	expected_thread_id: OTHER_THREAD,
};

test('a run that resumed the declared thread exits 0 and the verdict says it was never run live', () => {
	const directory = runDirectory(RESUME_RECORD, resumeRun());
	const { exitCode, verdictPath } = runMovedCommand(['--run', directory]);
	expect(exitCode).toBe(0);
	expect(verdictPath).toBe(join(directory, MOVED_VERDICT_FILE));
	expect(JSON.parse(readFileSync(verdictPath, 'utf8'))).toMatchObject({
		status: 'pass',
		kind: 'resume',
		violations: [],
	});
});

test('a wrong resume and a tier left unchanged exit 1 with their violations', () => {
	const resumed = runMovedCommand([
		'--run',
		runDirectory(RESUME_RECORD, resumeRun({ pickedThread: 'x' })),
	]);
	expect(resumed.exitCode).toBe(1);
	const tier = runMovedCommand([
		'--run',
		runDirectory(
			{ kind: 'tier', wire_recording: 'wire.jsonl', wire_value: 'priority' },
			tierRun({ tier: null }),
		),
	]);
	expect(tier.exitCode).toBe(1);
	expect(JSON.parse(readFileSync(tier.verdictPath, 'utf8'))).toMatchObject({
		status: 'fail',
		violations: [{ journey: 'tier' }],
	});
});

test('a verdict is never overwritten and the run path must be absolute', () => {
	const directory = runDirectory(RESUME_RECORD, resumeRun());
	runMovedCommand(['--run', directory]);
	expect(() => runMovedCommand(['--run', directory])).toThrow('already exists');
	expect(() => runMovedCommand(['--run', 'relative'])).toThrow('absolute');
});
