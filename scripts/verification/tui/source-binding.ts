import { createHash } from 'node:crypto';
import { readFileSync, realpathSync, statSync } from 'node:fs';
import { isAbsolute, relative, resolve, sep } from 'node:path';

import { z } from 'zod';

import { artifactBindingSchema, sourceBindingSchema } from './evidence-schema.ts';
import type { ArtifactBinding, CapturePlan, SourceBinding } from './evidence-schema.ts';
import { gitOutput } from './git-source.ts';

const jsonValueSchema = z.json();
function compareUnicodeCodepoints(left: string, right: string): number {
	const leftPoints = Array.from(left, (character) => character.codePointAt(0) ?? 0);
	const rightPoints = Array.from(right, (character) => character.codePointAt(0) ?? 0);
	const limit = Math.min(leftPoints.length, rightPoints.length);
	for (let index = 0; index < limit; index += 1) {
		const difference = (leftPoints[index] ?? 0) - (rightPoints[index] ?? 0);
		if (difference !== 0) return difference;
	}
	return leftPoints.length - rightPoints.length;
}
export function canonicalJson(value: unknown): string {
	const parsed = jsonValueSchema.parse(value);
	if (Array.isArray(parsed)) return `[${parsed.map(canonicalJson).join(', ')}]`;
	if (parsed !== null && typeof parsed === 'object')
		return `{${Object.keys(parsed)
			.toSorted(compareUnicodeCodepoints)
			.map((key) => `${canonicalJson(key)}: ${canonicalJson(parsed[key])}`)
			.join(', ')}}`;
	return JSON.stringify(parsed).replace(
		/[\u0080-\uFFFF]/g,
		(character) => `\\u${character.charCodeAt(0).toString(16).padStart(4, '0')}`,
	);
}
export function fileDigest(path: string): string {
	return createHash('sha256').update(readFileSync(path)).digest('hex');
}
export function captureArtifact(path: string): ArtifactBinding {
	return artifactBindingSchema.parse({ path: realpathSync(path), sha256: fileDigest(path) });
}
export function verifyArtifact(binding: ArtifactBinding): void {
	if (realpathSync(binding.path) !== binding.path)
		throw new Error(`artifact location changed: ${binding.path}`);
	if (fileDigest(binding.path) !== binding.sha256)
		throw new Error(`artifact changed: ${binding.path}`);
}
export function requireDigest(path: string, expected: string): void {
	if (fileDigest(path) !== expected) throw new Error(`evidence digest mismatch: ${path}`);
}
export function isInside(root: string, path: string): boolean {
	const rel = relative(resolve(root), resolve(path));
	return !isAbsolute(rel) && rel !== '..' && !rel.startsWith(`..${sep}`);
}
export function sourceBinding(repository: string): SourceBinding {
	const names = gitOutput(repository, [
		'ls-files',
		'-z',
		'--cached',
		'--others',
		'--exclude-standard',
		'--',
		'crates',
		'Cargo.toml',
		'Cargo.lock',
		'.cargo',
		'scripts/verification/tui',
	])
		.split('\0')
		.filter(Boolean);
	const files: Record<string, string> = {};
	for (const name of [...new Set(names)].toSorted()) {
		const path = resolve(repository, name);
		try {
			if (statSync(path).isFile()) files[name] = fileDigest(path);
		} catch (error) {
			if (!isMissingFile(error)) throw error;
		}
	}
	return sourceBindingSchema.parse({
		head: gitOutput(repository, ['rev-parse', 'HEAD']).trim(),
		files,
	});
}
export function isMissingFile(error: unknown): boolean {
	return error instanceof Error && 'code' in error && error.code === 'ENOENT';
}
export function sourceDigest(binding: SourceBinding): string {
	return createHash('sha256').update(canonicalJson(binding)).digest('hex');
}
export function verifySourceFiles(repository: string, binding: SourceBinding): void {
	for (const [name, digest] of Object.entries(binding.files)) {
		const path = resolve(repository, name);
		if (
			isAbsolute(name) ||
			name.split(/[\\/]/u).includes('..') ||
			!isInside(realpathSync(repository), realpathSync(path)) ||
			fileDigest(path) !== digest
		)
			throw new Error(`source changed or unsafe: ${name}`);
	}
}
export function verifyTarget(plan: CapturePlan): void {
	if (canonicalJson(sourceBinding(plan.repository)) !== canonicalJson(plan.source))
		throw new Error('declared product source changed; stop and re-pin');
	for (const binding of [plan.node, plan.capture_driver, plan.harness_binary])
		verifyArtifact(binding);
	if (process.version !== 'v26.12.1') throw new Error('capture requires pinned Node v26.12.1');
	if (realpathSync(process.execPath) !== plan.node.path)
		throw new Error('parent CLI must use the declared Node executable');
	if (!statSync(plan.temporary_directory).isDirectory())
		throw new Error('declared existing private temporary directory required');
}
export function readJson(path: string): unknown {
	return JSON.parse(readFileSync(path, 'utf8'));
}
