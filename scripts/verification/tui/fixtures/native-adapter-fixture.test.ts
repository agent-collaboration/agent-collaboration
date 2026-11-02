import {
	existsSync,
	mkdtempSync,
	readFileSync,
	realpathSync,
	rmSync,
	writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { afterEach, expect, test, vi } from 'vitest';

import { nativeAdapterFixtureMain, prepareNativeAdapterFixture } from './native-adapter-fixture.ts';

// Model a release written before the native watcher starts: the filesystem is
// real, but its notification channel never calls the registered callback.
vi.mock('node:fs', async (importOriginal) => {
	const original = await importOriginal<typeof import('node:fs')>();
	return { ...original, watch: vi.fn(() => ({ close: vi.fn() })) };
});

const roots: string[] = [];
const originalExitCode = process.exitCode;
afterEach(() => {
	vi.clearAllTimers();
	vi.useRealTimers();
	vi.restoreAllMocks();
	vi.unstubAllEnvs();
	process.exitCode = originalExitCode;
	for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
});

test('pending_fixture_observes_exact_release_without_filesystem_notifications', () => {
	// Arrange the same pending child and real durable paths as the native test.
	const root = realpathSync(mkdtempSync(join(tmpdir(), 'native-adapter-release-')));
	roots.push(root);
	prepareNativeAdapterFixture(root, 'pending-valid', process.execPath);
	const receipt = join(root, 'output/fixture-finished');
	const release = join(root, 'output/fixture-release');
	vi.stubEnv('QUOTA_COMPONENT_FIXTURE', 'commit-pending');
	vi.stubEnv('QUOTA_COMPONENT_FIXTURE_RECEIPT_FILE', receipt);
	const output = vi.spyOn(process.stdout, 'write').mockReturnValue(true);
	vi.useFakeTimers();

	// Act: no watcher event is delivered, even after the exact release appears.
	nativeAdapterFixtureMain(['child', root]);
	expect(output).toHaveBeenCalledWith('NATIVE_ADAPTER_READY\n');
	writeFileSync(release, 'wrong\n');
	vi.advanceTimersByTime(100);
	expect(existsSync(receipt)).toBe(false);
	writeFileSync(release, 'settle\n');
	vi.advanceTimersByTime(100);

	// Assert the real receipt, not a callback count or an elapsed-time increase.
	expect(readFileSync(receipt, 'utf8')).toBe(
		'QUOTA_COMPONENT_FIXTURE_FINISHED scenario=CommitPending released=true external_requests=0 persisted_writes=0\n',
	);
	expect(process.exitCode).toBe(0);
});
