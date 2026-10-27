import { spawnSync } from 'node:child_process';
import { readlinkSync } from 'node:fs';

import { processIdentitySchema } from './evidence-schema.ts';
import type { ProcessIdentity } from './evidence-schema.ts';
export function lookupProcessIdentity(pid: number): ProcessIdentity | null {
	const result = spawnSync('/bin/ps', ['-p', String(pid), '-o', 'pid=,uid=,pgid=,lstart=,comm='], {
		encoding: 'utf8',
		timeout: 5000,
	});
	if (result.status === 1 && !result.stdout.trim() && !result.stderr.trim()) return null;
	if (result.error || result.status !== 0)
		throw new Error(`process identity unavailable: ${result.error?.message ?? result.stderr}`);
	const match = /^(\d+)\s+(\d+)\s+(\d+)\s+(\S+\s+\S+\s+\S+\s+\S+\s+\S+)\s+(.+)$/u.exec(
		result.stdout.trim(),
	);
	if (!match) throw new Error('incomplete current process identity');
	return processIdentitySchema.parse({
		pid: Number(match[1]),
		uid: Number(match[2]),
		group: Number(match[3]),
		birth: match[4]?.trim().replace(/\s+/gu, ' '),
		executable: process.platform === 'linux' ? readlinkSync(`/proc/${pid}/exe`) : match[5],
	});
}
export function sameIdentity(current: ProcessIdentity | null, recorded: ProcessIdentity): boolean {
	return (
		current !== null &&
		current.pid === recorded.pid &&
		current.uid === recorded.uid &&
		current.group === recorded.group &&
		current.birth === recorded.birth &&
		current.executable === recorded.executable
	);
}
