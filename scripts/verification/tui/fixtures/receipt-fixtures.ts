/** Protocol fixtures exercise collectors, never substitute fabricated bytes for UI proof. */
import { mkdirSync, mkdtempSync, realpathSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { capturePlanSchema } from '../evidence/evidence-schema.ts';
import type { CapturePlan } from '../evidence/evidence-schema.ts';
import { captureArtifact } from '../evidence/source-binding.ts';

export function temporaryRoot(): string {
	return realpathSync(mkdtempSync(join(tmpdir(), 'router-tui-contract-')));
}
export function saveJson(path: string, value: unknown): void {
	writeFileSync(path, JSON.stringify(value));
}
export function protocolPlan(root: string): CapturePlan {
	const binary = join(root, 'protocol-only-target');
	writeFileSync(binary, 'protocol only');
	const binding = captureArtifact(binary);
	const support = join(
		root,
		'crates/codex-router-quota-reset-test-harness/tests/quota_storyboard_capture_test',
	);
	mkdirSync(support, { recursive: true });
	writeFileSync(join(support, 'public_sessions_capture.rs'), 'protocol only');
	const child = join(root, 'scripts/verification/tui/fixtures/fixture-child.ts');
	mkdirSync(join(root, 'scripts/verification/tui/fixtures'), { recursive: true });
	writeFileSync(child, 'protocol only');
	return capturePlanSchema.parse({
		repository: root,
		output: join(root, 'output'),
		temporary_directory: root,
		source: { head: 'a'.repeat(40), files: { 'protocol-only-target': binding.sha256 } },
		node: captureArtifact(process.execPath),
		capture_driver: binding,
		harness_binary: binding,
		build_declaration: 'Protocol bytes only, not product or image proof',
	});
}
