import { expect, test } from 'vitest';

import { parseAdapterCommand, planTtydInvocation, TtydInvocationRejected } from './ttyd-adapter.ts';

const VHS_CAPTURE_ARGUMENTS = [
	'--port=48123',
	'--interface',
	'127.0.0.1',
	'-t',
	'rendererType=canvas',
	'-t',
	'disableResizeOverlay=true',
	'-t',
	'enableSixel=true',
	'-t',
	'customGlyphs=true',
	'--once',
	'--writable',
	'bash',
	'--noprofile',
	'--norc',
	'--login',
	'+o',
	'history',
];

function plan(vhsArguments: readonly string[]): ReturnType<typeof planTtydInvocation> {
	return planTtydInvocation({
		ttyd: '/declared/ttyd',
		vhsArguments,
		nodeExecutable: '/declared/node',
		fixtureChild: '/repository/fixture-child.ts',
		fixtureCommand: '/owned/output/fixture-command.json',
	});
}

test('version probe passes through to the declared ttyd', () => {
	expect(plan(['--version'])).toEqual({
		kind: 'version',
		arguments: ['/declared/ttyd', '--version'],
	});
});

test('capture keeps the port and ttyd options and replaces bash with the fixture child', () => {
	expect(plan(VHS_CAPTURE_ARGUMENTS)).toEqual({
		kind: 'capture',
		port: 48123,
		arguments: [
			'/declared/ttyd',
			'--port=48123',
			'--interface',
			'127.0.0.1',
			'-t',
			'rendererType=canvas',
			'-t',
			'disableResizeOverlay=true',
			'-t',
			'enableSixel=true',
			'-t',
			'customGlyphs=true',
			'--once',
			'--writable',
			'/declared/node',
			'/repository/fixture-child.ts',
			'/owned/output/fixture-command.json',
		],
	});
});

test('any other VHS ttyd invocation is rejected before exec', () => {
	for (const vhsArguments of [
		[],
		['--version', '--extra'],
		VHS_CAPTURE_ARGUMENTS.slice(1),
		VHS_CAPTURE_ARGUMENTS.slice(0, -1),
		[...VHS_CAPTURE_ARGUMENTS, '--extra'],
		VHS_CAPTURE_ARGUMENTS.map((argument) => (argument === '127.0.0.1' ? '0.0.0.0' : argument)),
		VHS_CAPTURE_ARGUMENTS.map((argument) => (argument === '--once' ? '--check-origin' : argument)),
		['--port', '48123', ...VHS_CAPTURE_ARGUMENTS.slice(1)],
	]) {
		expect(() => plan(vhsArguments)).toThrow('Rejected unexpected VHS ttyd invocation');
	}
});

test('capture port must be a decimal port number', () => {
	for (const port of ['', '0', '65536', '123456', '-1', '+80', '8o', '0x50', ' 80']) {
		expect(() => plan([`--port=${port}`, ...VHS_CAPTURE_ARGUMENTS.slice(1)])).toThrow(
			TtydInvocationRejected,
		);
	}
	expect(plan(['--port=65535', ...VHS_CAPTURE_ARGUMENTS.slice(1)])).toMatchObject({
		kind: 'capture',
		port: 65535,
	});
	expect(plan(['--port=1', ...VHS_CAPTURE_ARGUMENTS.slice(1)])).toMatchObject({
		kind: 'capture',
		port: 1,
	});
});

test('adapter command requires each named text field', () => {
	const command = {
		ttyd: '/declared/ttyd',
		output: '/owned/output',
		dynamic_library_path: '/owned/lib',
	};
	expect(parseAdapterCommand(command)).toEqual(command);
	for (const changed of [
		{ ...command, ttyd: '' },
		{ output: '/owned/output', dynamic_library_path: '/owned/lib' },
		{ ...command, output: 1 },
		[command],
		null,
	]) {
		expect(() => parseAdapterCommand(changed)).toThrow();
	}
});
