/** Split-source declarations are apparatus inputs, never product UI proof. */
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { afterEach, expect, test, vi } from 'vitest';

import {
	captureArtifact,
	fileDigest,
	sourceBinding,
	sourceDigest,
} from '../evidence/source-binding.ts';
import {
	initializedSourceRepository,
	controlledExecutable,
} from '../fixtures/controlled-child-fixtures.ts';
import { temporaryRoot } from '../fixtures/receipt-fixtures.ts';
import { PtyGridDriver } from '../terminal/pty-grid-driver.ts';
import { prepareOptionNavigation } from './prepare-option-navigation-plan.ts';
afterEach(() => vi.unstubAllEnvs());

test('paired plan retains separate product and driver sources and rejects mismatched artifacts', () => {
	const productRepository = initializedSourceRepository();
	const driverRepository = initializedSourceRepository();
	const root = temporaryRoot();
	const productPath = join(root, 'product');
	const driverPath = join(root, 'driver');
	writeFileSync(productPath, 'product-only artifact');
	writeFileSync(driverPath, 'streaming driver artifact');
	const productSource = sourceBinding(productRepository),
		driverSource = sourceBinding(driverRepository);
	const snapshot = join(root, 'source.json'),
		diff = join(root, 'consumer.patch');
	writeFileSync(snapshot, JSON.stringify(productSource));
	writeFileSync(diff, 'controlled source declaration');
	const productManifest = {
		sourceHead: productSource.head,
		sourceSnapshotPath: snapshot,
		sourceSnapshotFileSha256: fileDigest(snapshot),
		sourceCanonicalSha256: sourceDigest(productSource),
		consumerDiffPath: diff,
		consumerDiffSha256: fileDigest(diff),
		product: captureArtifact(productPath),
		requiredCoreHead: '3396fe0d84c1acd233b14dcfa5a0b192e5df287c',
		marker: { contract: 'OSC7770' },
		fixtureSurface: 'sessions-filters',
	};
	// prepare resolves a product declaration's tmp/s6b location, as on S6b.
	mkdirSync(join(productRepository, 'tmp/s6b'), { recursive: true });
	const manifest = join(productRepository, 'tmp/s6b/product.json');
	writeFileSync(manifest, JSON.stringify(productManifest));
	const driverManifest = join(root, 'driver.json');
	writeFileSync(
		driverManifest,
		JSON.stringify({
			repository: driverRepository,
			source: driverSource,
			sourceDigest: sourceDigest(driverSource),
			driver: captureArtifact(driverPath),
		}),
	);
	const args = [manifest, driverManifest, join(root, 'run'), process.execPath];
	const plan = prepareOptionNavigation(args, driverRepository);
	expect(plan.repository).toBe(productRepository);
	expect(plan.productSourceDigest).toBe(sourceDigest(productSource));
	expect(plan.driverSourceDigest).toBe(sourceDigest(driverSource));
	expect(plan.driverSource.repository).toBe(driverRepository);
	expect(plan.product.path).toBe(productPath);
	expect(plan.driver.path).toBe(driverPath);
	writeFileSync(driverPath, 'wrong driver artifact');
	expect(() => prepareOptionNavigation(args, driverRepository)).toThrow('artifact changed');
});

test('driver forwards the explicit product path and digest while dropping ambient nominations', async () => {
	vi.stubEnv('TUI_GRID_PRODUCT_BINARY', '/ambient/must-not-flow');
	vi.stubEnv('TUI_GRID_PRODUCT_SHA256', 'ambient-must-not-flow');
	const root = temporaryRoot();
	const observed = join(root, 'observed.json');
	const product = join(root, 'declared-product');
	writeFileSync(product, 'declared product');
	const binary = controlledExecutable(
		root,
		'nomination-control',
		`
const fs=require('node:fs');fs.writeFileSync(${JSON.stringify(observed)},JSON.stringify({path:process.env.TUI_GRID_PRODUCT_BINARY,sha256:process.env.TUI_GRID_PRODUCT_SHA256}));
process.stdout.write('TUI_GRID READY 120 36 STREAM\\n');
process.stdin.on('data',()=>{process.stdout.write('TUI_GRID STOPPED 0\\n');process.exit(0);});`,
	);
	const artifact = captureArtifact(product);
	await [artifact, undefined].reduce<Promise<void>>(async (previous, nominated) => {
		await previous;
		const driver = await PtyGridDriver.start({
			repository: root,
			driver: binary,
			surface: 'sessions-filters',
			keys: 'legacy',
			evidence: root,
			...(nominated === undefined ? {} : { product: nominated }),
		});
		try {
			const actual: unknown = JSON.parse(readFileSync(observed, 'utf8'));
			expect(actual).toEqual(
				nominated === undefined ? {} : { path: artifact.path, sha256: artifact.sha256 },
			);
		} finally {
			await driver.stop();
		}
	}, Promise.resolve());
});
