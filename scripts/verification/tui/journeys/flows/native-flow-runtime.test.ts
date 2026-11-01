/** Controlled child qualifies the native adapter, never a product UI observation. */
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

import { expect, test } from 'vitest';

import { controlledExecutable } from '../../fixtures/controlled-child-fixtures.ts';
import { temporaryRoot } from '../../fixtures/receipt-fixtures.ts';
import type { InputConsumptionExpectation } from '../../sessions/input-consumption-schema.ts';
import { gridLines } from '../../terminal/terminal-grid.ts';
import { absentAtBase, flowReference } from './flow-definition.ts';
import type { FlowJourney } from './flow-definition.ts';
import { executeNativeFlow } from './native-flow-runtime.ts';
import type { NativeFlowBindings, NativeFlowInputBinding } from './native-flow-runtime.ts';

const initial: InputConsumptionExpectation = {
	event: { kind: 'key', code: { kind: 'fixed', name: 'Esc' }, modifiers: [], phase: 'press' },
	surface: { kind: 'screen' },
	target: null,
	action: null,
};
const tab: InputConsumptionExpectation = {
	event: { kind: 'key', code: { kind: 'fixed', name: 'Tab' }, modifiers: [], phase: 'press' },
	surface: { kind: 'screen' },
	target: 'action:start',
	action: null,
};
const quit: InputConsumptionExpectation = {
	event: {
		kind: 'key',
		code: { kind: 'shortcut', key: 'c' },
		modifiers: ['control'],
		phase: 'press',
	},
	surface: { kind: 'screen' },
	target: null,
	action: 'quit',
};
const journey: FlowJourney = {
	journeyId: 'native-adapter-control',
	requirements: ['U12/R13'],
	surface: 'sessions',
	availability: absentAtBase('S6'),
	reference: flowReference('R13', 'U12'),
	grids: ['120x36'],
	keys: ['legacy'],
	fixtures: [],
	steps: [
		{ kind: 'focus', target: 'Start', via: 'Tab' },
		{ kind: 'click', target: 'Start' },
		{ kind: 'readback', port: 'preparation', checks: ['compound-start-once'] },
	],
	wrongOutcomes: ['unrelated later frame replaces marked result', 'wrong target launches'],
	admissionInputs: [],
};
function setup(mode: 'right' | 'wrong-frame' | 'missing-binding' | 'exit-step' | 'exit-click'): {
	bindings: NativeFlowBindings;
	directory: string;
} {
	const directory = temporaryRoot(),
		receipt = join(directory, 'input-dispatch.jsonl');
	const executable = controlledExecutable(
		directory,
		'native-runtime-control',
		`
const fs=require('node:fs'),readline=require('node:readline');let sequence=0,starts=0;
const receipt=${JSON.stringify(receipt)},mode=${JSON.stringify(mode)};fs.writeFileSync(receipt,'');
const emit=value=>process.stdout.write('TUI_GRID '+value+'\\n');
const frame=value=>'\\x1b[?2026h\\x1b[2J\\x1b[H'+value+'\\x1b[?2026l';
const bytes=value=>emit('BYTES '+Buffer.from(value).toString('hex'));
emit('READY 120 36 STREAM');bytes(frame('界 Start')+'\\x1b[?1000h\\x1b[?1006h');
readline.createInterface({input:process.stdin}).on('line',line=>{
if(line==='stop'){emit('EOF');process.stdout.write('TUI_GRID STOPPED 0\\n',()=>process.exit(0));return;}
if(line.startsWith('reply ')){emit('OK');return;}
if(!line.startsWith('write '))throw new Error('unexpected control command '+line);
const hex=line.slice(6),text=Buffer.from(hex,'hex').toString();let event,target=null,action=null,ending=false;
if(hex==='1b') event={kind:'key',code:{kind:'fixed',name:'Esc'},modifiers:[],phase:'press'};
else if(hex==='09'){event={kind:'key',code:{kind:'fixed',name:'Tab'},modifiers:[],phase:'press'};target='action:start';}
else if(hex==='03'){event={kind:'key',code:{kind:'shortcut',key:'c'},modifiers:['control'],phase:'press'};action='quit';ending=true;}
else {
 const match=/^\\x1b\\[<0;4;1([Mm])$/.exec(text);if(!match)throw new Error('wrong painted click '+JSON.stringify(text));
 event={kind:'mouse',mouse:{kind:match[1]==='M'?'down':'up',button:'left'},column:3,row:0,modifiers:[]};target='action:start';
 if(match[1]==='M'){action='start';starts++;if(mode==='exit-click')ending=true;}
}
sequence++;fs.appendFileSync(receipt,JSON.stringify({sequence,event,surface:{kind:'screen'},target,action})+'\\n');emit('OK');
if(ending){fs.writeFileSync(${JSON.stringify(join(directory, 'effects.json'))},JSON.stringify({starts}));emit('EOF');process.stdout.write('TUI_GRID FINISHED 0\\n',()=>process.exit(0));return;}
const label=mode==='wrong-frame'&&sequence===2?'WRONG':'界 Start';
bytes(frame(label+'\\r\\n'+(sequence>=2?'❯ Start':'ready'))+'\\x1b]7770;dispatch='+sequence+'\\x1b\\\\'+frame('unrelated later'));
});`,
	);
	const inputs = new Map<number, NativeFlowInputBinding>([
		[
			0,
			{
				kind: 'focus',
				route: [{ key: 'Tab', expected: [tab] }],
				isFocused: (grid) => gridLines(grid)[1]?.trim() === '❯ Start',
			},
		],
		[
			1,
			{
				kind: 'click',
				expected: (location) => [
					{
						event: {
							kind: 'mouse',
							mouse: { kind: 'down', button: 'left' },
							column: location.column,
							row: location.row,
							modifiers: [],
						},
						surface: { kind: 'screen' },
						target: 'action:start',
						action: 'start',
					},
					{
						event: {
							kind: 'mouse',
							mouse: { kind: 'up', button: 'left' },
							column: location.column,
							row: location.row,
							modifiers: [],
						},
						surface: { kind: 'screen' },
						target: 'action:start',
						action: null,
					},
				],
			},
		],
	]);
	if (mode === 'missing-binding') inputs.delete(1);
	if (mode === 'exit-click') {
		const click = inputs.get(1);
		if (click?.kind !== 'click') throw new Error('controlled click absent');
		inputs.set(1, {
			...click,
			endsOnPress: true,
			expected: (point) => click.expected(point).slice(0, 1),
		});
	}
	if (mode === 'exit-step') {
		inputs.clear();
		inputs.set(0, { kind: 'exit', expected: [quit] });
	}
	return {
		directory,
		bindings: {
			target: {
				repository: directory,
				driver: executable,
				surface: 'sessions-filters',
				keys: 'legacy',
				evidence: directory,
			},
			receipt,
			original: {
				chrome: '/unneeded/no-capture',
				sourceHead: 'a'.repeat(40),
				binarySha256: 'b'.repeat(64),
			},
			ready: (grid) => gridLines(grid)[0]?.trim() === '界 Start',
			initialBarrier: { key: 'Escape', expected: [initial] },
			resizeExpected: () => {
				throw new Error('unexpected resize');
			},
			inputs,
			targets: new Map([
				['Start', () => ({ label: 'Start', bounds: { left: 0, top: 0, right: 32, bottom: 1 } })],
			]),
			fixtures: new Map(),
			readbacks: new Map([
				['preparation', async (context) => ({ line: gridLines(context.frame.grid)[0]?.trim() })],
			]),
			checks: new Map([
				[
					'compound-start-once',
					async (_context, observed) =>
						JSON.stringify(observed) === JSON.stringify({ line: '界 Start' })
							? []
							: ['wrong marked grid'],
				],
			]),
			quit: { key: 'Ctrl+C', expected: [quit] },
		},
	};
}
test('native executor focuses and clicks the marked painted target despite later unrelated frames', async () => {
	const { bindings, directory } = setup('right');
	const result = await executeNativeFlow({ journey, grid: '120x36', keys: 'legacy' }, bindings);
	expect(result.kind).toBe('completed');
	if (result.kind !== 'completed') throw new Error('controlled binding gap');
	expect(result.steps.map((step) => step.dispatchSequences)).toEqual([[2], [3, 4], [4]]);
	expect(result.steps[1]?.inputObservation).toEqual({ kind: 'after-burst', sequence: 4 });
	expect(result.finish.dispatchSequences).toEqual([5]);
	expect(JSON.parse(readFileSync(join(directory, 'effects.json'), 'utf8'))).toEqual({ starts: 1 });
	expect(readFileSync(join(directory, 'terminal.ansi'), 'utf8')).toContain('unrelated later');
});

test('terminal pointer activation settles through exact press ACK and exit, with no continuing frame', async () => {
	const { bindings, directory } = setup('exit-click');
	const terminalJourney: FlowJourney = { ...journey, steps: journey.steps.slice(0, 2) };
	const result = await executeNativeFlow(
		{ journey: terminalJourney, grid: '120x36', keys: 'legacy' },
		bindings,
	);
	if (result.kind !== 'completed') throw new Error('controlled terminal click admission gap');
	expect(result.steps[1]?.dispatchSequences).toEqual([3]);
	expect(result.steps[1]?.inputObservation).toBeUndefined();
	expect(result.finish.dispatchSequences).toEqual([3]);
	expect(JSON.parse(readFileSync(join(directory, 'effects.json'), 'utf8'))).toEqual({ starts: 1 });
});
test('an absent late input binding rejects before launching a controlled child', async () => {
	const { bindings, directory } = setup('missing-binding');
	expect(await executeNativeFlow({ journey, grid: '120x36', keys: 'legacy' }, bindings)).toEqual({
		kind: 'fixture-gap',
		gaps: [{ kind: 'input', name: 'step 1' }],
	});
	expect(existsSync(bindings.receipt)).toBe(false);
	expect(existsSync(join(directory, 'driver.stdout'))).toBe(false);
});
test('wrong marked text cannot borrow a valid target from a later or earlier grid', async () => {
	const { bindings, directory } = setup('wrong-frame');
	await expect(
		executeNativeFlow({ journey, grid: '120x36', keys: 'legacy' }, bindings),
	).rejects.toThrow('0 visible matches');
	expect(readFileSync(join(directory, 'driver.stdout'), 'utf8')).toContain('STOPPED');
});
test('a declared terminating authored input settles without a frame or duplicate Quit', async () => {
	const { bindings, directory } = setup('exit-step');
	const result = await executeNativeFlow(
		{
			journey: { ...journey, steps: [{ kind: 'press', key: 'Ctrl+C' }] },
			grid: '120x36',
			keys: 'legacy',
		},
		bindings,
	);
	expect(result.kind).toBe('completed');
	expect(
		readFileSync(join(directory, 'input-dispatch.jsonl'), 'utf8').trim().split('\n'),
	).toHaveLength(2);
});
test('a per-input press assertion cannot be admitted as a multi-input burst', async () => {
	const { bindings } = setup('exit-step');
	const bad = {
		...bindings,
		inputs: new Map<number, NativeFlowInputBinding>([
			[0, { kind: 'exit', expected: [quit, quit] }],
		]),
	};
	await expect(
		executeNativeFlow(
			{
				journey: { ...journey, steps: [{ kind: 'press', key: 'Ctrl+C' }] },
				grid: '120x36',
				keys: 'legacy',
			},
			bad,
		),
	).rejects.toThrow('exactly one input ACK');
	expect(existsSync(bindings.receipt)).toBe(false);
});
