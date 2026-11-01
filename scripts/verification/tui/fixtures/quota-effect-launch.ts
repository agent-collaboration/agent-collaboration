/** Admit the declared S7 CLI libtest on the inherited PTY; no product is started here. */
import { accessSync, constants, lstatSync, readFileSync, realpathSync } from 'node:fs';
import { basename, dirname, isAbsolute, join } from 'node:path';

import { z } from 'zod';

import { fileDigest, verifyArtifact } from '../evidence/source-binding.ts';
import {
	quotaEffectArguments,
	quotaEffectConfigurationSchema,
	quotaLoginExit,
} from '../journeys/flows/quota-fixture-contract.ts';
import type { ChildLaunch, FixtureCommand } from './fixture-child.ts';

const digestSchema = z.string().regex(/^[a-f0-9]{64}$/u);
const launchRecordSchema = z.strictObject({
	surface: z.literal('quota-effects'),
	binary: z.string(),
	binary_sha256: digestSchema,
	approved_cwd: z.string(),
	output: z.string(),
	effect_configuration: z.string(),
	effect_configuration_sha256: digestSchema,
	login_standin_sha256: digestSchema.optional(),
	login_path_prefix: z.string().optional(),
});

function requireDirectory(path: string): void {
	const metadata = lstatSync(path);
	if (
		!isAbsolute(path) ||
		realpathSync(path) !== path ||
		!metadata.isDirectory() ||
		metadata.isSymbolicLink() ||
		metadata.uid !== process.getuid?.()
	)
		throw new Error('quota fixture directory must be canonical and owned');
}
function requireFuturePath(root: string, path: string): void {
	if (!isAbsolute(path) || dirname(path) !== root)
		throw new Error('quota outputs must be direct children of the private output');
	try {
		lstatSync(path);
	} catch (error) {
		if (error instanceof Error && 'code' in error && error.code === 'ENOENT') return;
		throw error;
	}
	throw new Error('quota output already exists');
}

export function quotaEffectLaunch(configuration: FixtureCommand): ChildLaunch {
	const record = launchRecordSchema.parse(configuration);
	requireDirectory(record.output);
	requireDirectory(record.approved_cwd);
	if ((lstatSync(record.output).mode & 0o077) !== 0 || !record.output.startsWith('/private/tmp/'))
		throw new Error('quota fixture output must be private mode 0700 under /private/tmp');
	if (
		dirname(record.effect_configuration) !== record.output ||
		realpathSync(record.effect_configuration) !== record.effect_configuration ||
		fileDigest(record.effect_configuration) !== record.effect_configuration_sha256
	)
		throw new Error('quota effect configuration hash or location changed');
	const effect = quotaEffectConfigurationSchema.parse(
		JSON.parse(readFileSync(record.effect_configuration, 'utf8')),
	);
	if (effect.outputDirectory !== record.output)
		throw new Error('quota config output differs from declared output');
	const paths = [effect.events, effect.receipt, effect.controlSocket, effect.dispatchReceipt];
	if ('loginReceipt' in effect) paths.push(effect.loginReceipt, effect.loginGateSocket);
	if (new Set(paths).size !== paths.length || paths.includes(record.effect_configuration))
		throw new Error('quota output paths collide');
	for (const path of paths) requireFuturePath(record.output, path);
	for (const path of [
		effect.controlSocket,
		...('loginGateSocket' in effect ? [effect.loginGateSocket] : []),
	])
		if (Buffer.byteLength(path, 'utf8') > 103)
			throw new Error('quota Unix socket path exceeds macOS limit');
	verifyArtifact({ path: record.binary, sha256: record.binary_sha256 });
	accessSync(record.binary, constants.X_OK);
	const environment: Record<string, string> = {
		TERM: 'xterm-256color',
		LANG: 'C.UTF-8',
		TMPDIR: '/private/tmp',
		QUOTA_EFFECT_FIXTURE_CONFIG: record.effect_configuration,
	};
	for (const name of ['HOME', 'PATH', 'LC_ALL', 'DYLD_LIBRARY_PATH', 'LD_LIBRARY_PATH']) {
		const value = process.env[name];
		if (value !== undefined) environment[name] = value;
	}
	const loginExit = quotaLoginExit(effect.scenario);
	if (loginExit === null) {
		if (record.login_standin_sha256 !== undefined || record.login_path_prefix !== undefined)
			throw new Error('non-login quota scenario forbids login launch fields');
	} else {
		if (
			!('loginStandin' in effect) ||
			record.login_standin_sha256 === undefined ||
			record.login_path_prefix === undefined
		)
			throw new Error(
				'login quota scenario requires its declared stand-in and private PATH prefix',
			);
		verifyArtifact({ path: effect.loginStandin, sha256: record.login_standin_sha256 });
		accessSync(effect.loginStandin, constants.X_OK);
		if (basename(effect.loginStandin) !== 'quota-login-recording-standin')
			throw new Error('quota login stand-in basename changed');
		const prefix = record.login_path_prefix;
		requireDirectory(prefix);
		if (
			dirname(prefix) !== record.output ||
			(lstatSync(prefix).mode & 0o077) !== 0 ||
			realpathSync(join(prefix, 'codex-router')) !== effect.loginStandin
		)
			throw new Error('quota login PATH must resolve exactly to its declared stand-in');
		environment['PATH'] =
			prefix + (environment['PATH'] === undefined ? '' : ':' + environment['PATH']);
		environment['QUOTA_LOGIN_STANDIN_RECEIPT'] = effect.loginReceipt;
		environment['QUOTA_LOGIN_STANDIN_GATE'] = effect.loginGateSocket;
		environment['QUOTA_LOGIN_STANDIN_EXIT'] = String(loginExit);
	}
	return {
		executable: record.binary,
		arguments: quotaEffectArguments,
		environment,
		cwd: record.approved_cwd,
	};
}
