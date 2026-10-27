import { execFileSync } from 'node:child_process';
import { chmodSync, mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
export function controlledExecutable(root: string, name: string, source: string): string {
	const path = join(root, name);
	writeFileSync(path, `#!${process.execPath}\n${source}\n`);
	chmodSync(path, 0o700);
	return path;
}
export function initializedSourceRepository(): string {
	const base =
		process.platform === 'darwin'
			? '/private/tmp/2026-10-quota-capture-tools/ts-test-fixtures'
			: join(tmpdir(), '2026-10-quota-capture-tools/ts-test-fixtures');
	mkdirSync(base, { recursive: true });
	const root = mkdtempSync(join(base, 'source-'));
	execFileSync('git', ['-C', root, 'init', '--quiet']);
	mkdirSync(join(root, 'crates'));
	writeFileSync(join(root, 'crates/tracked.rs'), 'tracked source');
	execFileSync('git', ['-C', root, 'add', '--', 'crates/tracked.rs']);
	execFileSync('git', [
		'-C',
		root,
		'-c',
		'user.name=TUI Contract Fixture',
		'-c',
		'user.email=fixture@example.invalid',
		'-c',
		'commit.gpgsign=false',
		'commit',
		'--allow-empty',
		'--no-gpg-sign',
		'--quiet',
		'-m',
		'isolated source fixture',
	]);
	return root;
}
