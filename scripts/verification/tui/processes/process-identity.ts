import { spawnSync } from 'node:child_process';
import { readlinkSync, realpathSync } from 'node:fs';

import { processIdentitySchema } from '../evidence/evidence-schema.ts';
import type { ProcessIdentity } from '../evidence/evidence-schema.ts';
/** PID, real then effective UID, group, start time and executable of one live process. */
export function lookupProcessIdentity(pid: number): ProcessIdentity | null {
	const result = spawnSync(
		'/bin/ps',
		['-p', String(pid), '-o', 'pid=,ruid=,uid=,pgid=,lstart=,comm='],
		{ encoding: 'utf8', timeout: 5000 },
	);
	if (result.status === 1 && !result.stdout.trim() && !result.stderr.trim()) return null;
	if (result.error || result.status !== 0)
		throw new Error(`process identity unavailable: ${result.error?.message ?? result.stderr}`);
	const match = /^(\d+)\s+(\d+)\s+(\d+)\s+(\d+)\s+(\S+\s+\S+\s+\S+\s+\S+\s+\S+)\s+(.+)$/u.exec(
		result.stdout.trim(),
	);
	if (!match) throw new Error('incomplete current process identity');
	return processIdentitySchema.parse({
		pid: Number(match[1]),
		realUid: Number(match[2]),
		effectiveUid: Number(match[3]),
		group: Number(match[4]),
		birth: match[5]?.trim().replace(/\s+/gu, ' '),
		executable: process.platform === 'linux' ? readlinkSync(`/proc/${pid}/exe`) : match[6],
	});
}
/** Same process: effective UID is evidence, not identity, because setuid utilities change it. */
export function sameIdentity(current: ProcessIdentity | null, recorded: ProcessIdentity): boolean {
	return (
		current !== null &&
		current.pid === recorded.pid &&
		current.realUid === recorded.realUid &&
		current.group === recorded.group &&
		current.birth === recorded.birth &&
		current.executable === recorded.executable
	);
}

/** Portable PTY children have no wrapper PID file; bind the one exact direct product child. */
export function requireOwnedProductChild(parentPid: number, productPath: string): ProcessIdentity {
	const inventory = spawnSync('/bin/ps', ['-axo', 'pid=,ppid='], {
		encoding: 'utf8',
		timeout: 5000,
	});
	if (inventory.error || inventory.status !== 0)
		throw new Error(
			'product child inventory unavailable: ' + (inventory.error?.message ?? inventory.stderr),
		);
	const children = inventory.stdout
		.trim()
		.split('\n')
		.flatMap((line) => {
			const [pid, parent] = line.trim().split(/\s+/u).map(Number);
			if (parent !== parentPid || pid === undefined) return [];
			const identity = lookupProcessIdentity(pid);
			return identity !== null &&
				identity.realUid === process.getuid?.() &&
				realpathSync(identity.executable) === productPath
				? [identity]
				: [];
		});
	const child = children[0];
	if (children.length !== 1 || child === undefined)
		throw new Error('one exact owned driver product child required');
	return child;
}
