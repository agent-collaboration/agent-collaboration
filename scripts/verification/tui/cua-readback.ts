import { readFileSync, realpathSync } from 'node:fs';
import { join, resolve } from 'node:path';

import { cuaReadbackSchema, cuaReadySchema } from './evidence-schema.ts';
import type { CapturePlan, CuaPtyReadback, TerminalViewport } from './evidence-schema.ts';
import { lookupProcessIdentity, sameIdentity } from './process-identity.ts';
import { fileDigest, readJson, sourceDigest, verifyTarget } from './source-binding.ts';
export function requireSamePty(after: CuaPtyReadback, before: CuaPtyReadback): void {
	if (
		after.node_sha256 !== before.node_sha256 ||
		after.surface !== before.surface ||
		after.source_sha256 !== before.source_sha256 ||
		after.binary_sha256 !== before.binary_sha256 ||
		!sameIdentity(after.identity, before.identity)
	)
		throw new Error('CUA resize replaced the declared PTY or target');
}
export function readCuaState(
	plan: CapturePlan,
	surface: 'sessions' | 'quota',
	expected: TerminalViewport,
): CuaPtyReadback {
	verifyTarget(plan);
	const folder = join(plan.output, `cua-${surface}`);
	const ready = cuaReadySchema.parse(readJson(join(folder, 'cua-ready.json')));
	if (
		ready.node_sha256 !== plan.node.sha256 ||
		ready.surface !== surface ||
		ready.binary_sha256 !== plan.harness_binary.sha256 ||
		ready.ttyd_sha256 !== plan.ttyd.sha256 ||
		resolve(ready.child_pid_file) !== resolve(folder, 'quota-child.pid') ||
		resolve(ready.viewport_file) !== resolve(folder, 'quota-child.viewport') ||
		resolve(ready.stop_file) !== resolve(folder, 'cua-stop') ||
		resolve(ready.artifact_export.directory) !== resolve(folder, 'original-images') ||
		JSON.stringify([...ready.artifact_export.scenes].toSorted()) !==
			JSON.stringify([...plan.cua_artifact_scenes].toSorted()) ||
		ready.artifact_export.source_sha256 !==
			fileDigest(
				join(
					plan.repository,
					'crates/codex-router-quota-reset-test-harness/tests/quota_storyboard_capture_test/cua_artifact_sink.rs',
				),
			)
	)
		throw new Error('CUA ready record does not match this declared fixture');
	const ttyd = lookupProcessIdentity(ready.ttyd_pid);
	const pid = Number(readFileSync(ready.child_pid_file, 'utf8').trim());
	if (!Number.isInteger(pid) || pid <= 0) throw new Error('incomplete child PID');
	const child = lookupProcessIdentity(pid);
	if (
		child === null ||
		ttyd === null ||
		child.uid !== process.getuid?.() ||
		ttyd.uid !== process.getuid?.() ||
		realpathSync(child.executable) !== plan.harness_binary.path ||
		realpathSync(ttyd.executable) !== plan.ttyd.path
	)
		throw new Error('CUA process identity does not match declared executables/owner');
	const values = readFileSync(ready.viewport_file, 'utf8').trim().split(/\s+/u);
	if (values.length !== 2) throw new Error('CUA viewport readback incomplete');
	const columns = Number(values[0]);
	const rows = Number(values[1]);
	if (columns !== expected.columns || rows !== expected.rows)
		throw new Error('actual PTY viewport differs from expected');
	return cuaReadbackSchema.parse({
		node_sha256: plan.node.sha256,
		surface,
		identity: child,
		viewport: { columns, rows },
		source_sha256: sourceDigest(plan.source),
		binary_sha256: plan.harness_binary.sha256,
		observed_at: new Date().toISOString(),
	});
}
