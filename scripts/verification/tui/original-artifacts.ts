import { readdirSync, readFileSync, existsSync } from 'node:fs';
import { join, resolve } from 'node:path';

import { cuaSavedArtifactSchema } from './evidence-schema.ts';
import type { CapturePlan } from './evidence-schema.ts';
import { readJson, requireDigest } from './source-binding.ts';
export function requireExactSet(
	actual: readonly string[],
	expected: readonly string[],
	message: string,
): void {
	if (
		actual.length !== expected.length ||
		new Set(actual).size !== actual.length ||
		[...actual].toSorted().some((value, index) => value !== [...expected].toSorted()[index])
	)
		throw new Error(message);
}
export function requireExactOrder(
	actual: readonly string[],
	expected: readonly string[],
	message: string,
): void {
	if (actual.length !== expected.length || actual.some((value, index) => value !== expected[index]))
		throw new Error(message);
}
export function requireOriginalPng(path: string): void {
	const bytes = readFileSync(path);
	if (
		bytes.length <= 1000 ||
		!bytes.subarray(0, 8).equals(Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]))
	)
		throw new Error('missing original PNG');
}
export interface OriginalCuaArtifact {
	readonly path: string;
	readonly scene: string;
	readonly bytes: number;
	readonly sha256: string;
	readonly encoding_round_trip: true;
	readonly mime_type: 'image/jpeg';
	readonly result: 'original-bytes-retained-ui-judgment-separate';
}
export function collectCuaArtifacts(
	plan: CapturePlan,
	surface: string,
	expectedCount: number,
): OriginalCuaArtifact[] {
	const folder = join(plan.output, `cua-${surface}`, 'original-images');
	const artifacts: OriginalCuaArtifact[] = [];
	for (const scene of plan.cua_artifact_scenes) {
		const record = join(folder, `${scene}.receipt.json`);
		if (!existsSync(record)) continue;
		const receipt = cuaSavedArtifactSchema.parse(readJson(record));
		if (receipt.scene !== scene || receipt.filename !== `${scene}.jpg`)
			throw new Error('original-artifact receipt scene/filename mismatch');
		const original = join(folder, receipt.filename);
		const bytes = readFileSync(original);
		requireDigest(original, receipt.sha256);
		if (
			bytes.length !== receipt.bytes ||
			!bytes.subarray(0, 2).equals(Buffer.from([255, 216])) ||
			!bytes.subarray(-2).equals(Buffer.from([255, 217]))
		)
			throw new Error('original-artifact bytes or JPEG format mismatch');
		artifacts.push({
			path: resolve(original),
			scene,
			bytes: receipt.bytes,
			sha256: receipt.sha256,
			encoding_round_trip: true,
			mime_type: 'image/jpeg',
			result: 'original-bytes-retained-ui-judgment-separate',
		});
	}
	if (expectedCount !== plan.cua_artifact_scenes.length || artifacts.length !== expectedCount)
		throw new Error('original-artifact count or declared file set mismatch');
	const files = readdirSync(folder);
	requireExactSet(
		files.filter((name) => name.endsWith('.jpg')),
		plan.cua_artifact_scenes.map((scene) => `${scene}.jpg`),
		'original-artifact count or declared file set mismatch',
	);
	requireExactSet(
		files.filter((name) => name.endsWith('.receipt.json')),
		plan.cua_artifact_scenes.map((scene) => `${scene}.receipt.json`),
		'original-artifact count or declared file set mismatch',
	);
	return artifacts;
}
export function findReceiptPaths(root: string, filename: string): string[] {
	const result: string[] = [];
	for (const entry of readdirSync(root, { withFileTypes: true })) {
		if (entry.isDirectory()) result.push(...findReceiptPaths(join(root, entry.name), filename));
		else if (entry.name === filename) result.push(join(root, entry.name));
	}
	return result;
}
