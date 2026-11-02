import { rmSync, mkdirSync, writeFileSync, symlinkSync } from 'node:fs';
import { join } from 'node:path';

import { afterEach, expect, test } from 'vitest';

import { initializedSourceRepository } from '../fixtures/controlled-child-fixtures.ts';
import { protocolPlan, temporaryRoot } from '../fixtures/receipt-fixtures.ts';
import { capturePlanSchema } from './evidence-schema.ts';
import {
	captureArtifact,
	canonicalJson,
	sourceBinding,
	sourceDigest,
	verifyArtifact,
	verifySourceFiles,
	verifyTarget,
} from './source-binding.ts';
const roots: string[] = [];
function rootDirectory(): string {
	const root = temporaryRoot();
	roots.push(root);
	return root;
}
afterEach(() => {
	for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
});
test('changed_artifact_is_rejected', () => {
	const root = rootDirectory();
	const path = join(root, 'binary');
	writeFileSync(path, 'declared build');
	const binding = captureArtifact(path);
	writeFileSync(path, 'replacement build');
	expect(() => verifyArtifact(binding)).toThrow('artifact changed');
});
test('source_binding_detects_changed_untracked_input', () => {
	const root = rootDirectory();
	const path = join(root, 'input.rs');
	writeFileSync(path, 'first');
	const binding = { head: 'a'.repeat(40), files: { 'input.rs': captureArtifact(path).sha256 } };
	verifySourceFiles(root, binding);
	writeFileSync(path, 'second');
	expect(() => verifySourceFiles(root, binding)).toThrow('source changed');
});
test('capture_target_rejects_omitted_or_changed_compiled_controls_input', () => {
	const repository = initializedSourceRepository();
	roots.push(repository);
	const root = rootDirectory();
	const support = join(repository, 'crates/capture');
	mkdirSync(support, { recursive: true });
	for (const name of ['image', 'checkpoint', 'controls'])
		writeFileSync(join(support, `grid_${name}.rs`), name);
	const plan = capturePlanSchema.parse({
		...protocolPlan(root),
		repository,
		source: sourceBinding(repository),
	});
	verifyTarget(plan);
	const controls = 'crates/capture/grid_controls.rs';
	const omitted = {
		...plan.source,
		files: Object.fromEntries(
			Object.entries(plan.source.files).filter(([name]) => name !== controls),
		),
	};
	expect(() => verifyTarget({ ...plan, source: omitted })).toThrow('source changed');
	writeFileSync(join(repository, controls), 'changed control fixture');
	expect(() => verifyTarget(plan)).toThrow('source changed');
});
test('source enumeration binds permanent package lock and excludes ignored dependencies', () => {
	const repository = initializedSourceRepository();
	roots.push(repository);
	mkdirSync(join(repository, 'scripts/verification/tui/node_modules'), { recursive: true });
	writeFileSync(join(repository, 'scripts/verification/tui/.gitignore'), 'node_modules/\n');
	writeFileSync(join(repository, 'scripts/verification/tui/package.json'), '{}');
	writeFileSync(join(repository, 'scripts/verification/tui/pnpm-lock.yaml'), 'lockfileVersion: 9');
	writeFileSync(join(repository, 'scripts/verification/tui/node_modules/ignored.js'), 'ignored');
	const binding = sourceBinding(repository);
	expect(binding.files).toHaveProperty('scripts/verification/tui/pnpm-lock.yaml');
	expect(Object.keys(binding.files).some((name) => name.includes('node_modules'))).toBe(false);
});
test('canonical JSON matches existing sorted spaced ASCII wire digest literal', () => {
	expect(canonicalJson({ ['z']: '東京', ['a']: { ['b']: 1, ['a']: [true, null] } })).toBe(
		'{"a": {"a": [true, null], "b": 1}, "z": "\\u6771\\u4eac"}',
	);
	expect(sourceDigest({ head: 'a'.repeat(40), files: { ['x']: 'b'.repeat(64) } })).toMatch(
		/^[a-f0-9]{64}$/u,
	);
});
test('source verification rejects traversal and symlink escape', () => {
	const root = rootDirectory();
	const outside = rootDirectory();
	writeFileSync(join(outside, 'secret'), 'outside');
	symlinkSync(join(outside, 'secret'), join(root, 'linked'));
	const digest = captureArtifact(join(outside, 'secret')).sha256;
	expect(() =>
		verifySourceFiles(root, { head: 'a'.repeat(40), files: { linked: digest } }),
	).toThrow('unsafe');
	expect(() =>
		verifySourceFiles(root, { head: 'a'.repeat(40), files: { '../outside': digest } }),
	).toThrow();
});
test('real Git enumeration binds both tracked and untracked source changes', () => {
	const repository = initializedSourceRepository();
	roots.push(repository);
	writeFileSync(join(repository, 'crates/untracked.rs'), 'untracked source');
	const binding = sourceBinding(repository);
	expect(binding.files).toHaveProperty('crates/tracked.rs');
	expect(binding.files).toHaveProperty('crates/untracked.rs');
	verifySourceFiles(repository, binding);
	writeFileSync(join(repository, 'crates/tracked.rs'), 'changed tracked source');
	expect(() => verifySourceFiles(repository, binding)).toThrow('source changed');
});
test('capture target binds exact current Node and rejects digest or executable substitution', () => {
	const repository = initializedSourceRepository();
	roots.push(repository);
	const root = rootDirectory();
	const plan = { ...protocolPlan(root), repository, source: sourceBinding(repository) };
	verifyTarget(plan);
	expect(() => verifyTarget({ ...plan, node: { ...plan.node, sha256: '0'.repeat(64) } })).toThrow(
		'artifact changed',
	);
	expect(() => verifyTarget({ ...plan, node: plan.capture_driver })).toThrow('parent CLI must use');
});
test('canonical wire digest escapes both halves of supplementary Unicode characters', () => {
	expect(canonicalJson('🐒')).toBe('"\\ud83d\\udc12"');
});
test('canonical wire digest sorts supplementary Unicode keys by codepoint', () => {
	expect(canonicalJson({ '🐒': 'second', '\ue000': 'first' })).toBe(
		'{"\\ue000": "first", "\\ud83d\\udc12": "second"}',
	);
});
