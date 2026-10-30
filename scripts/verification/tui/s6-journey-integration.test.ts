/** Observer apparatus only: controlled PTY protocol bytes are never product evidence. */
import { mkdirSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { afterEach, expect, test } from 'vitest';

import { baselinePlanSchema, observeMainEscape } from './baseline-journeys.ts';
import { controlledExecutable } from './controlled-child-fixtures.ts';
import { inputJourneyPlanSchema } from './input-journeys.ts';

const roots: string[] = [];
afterEach(() => {
	for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
});
const basePlan = {
	repository: '/declared/repository',
	productSourceHead: 'declared',
	driver: { path: '/declared/driver', sha256: 'a'.repeat(64) },
	product: { path: '/declared/product', sha256: 'b'.repeat(64) },
	output: '/declared/output',
};
test('picker-only plans require the local returned New request, without a preparer count', () => {
	const newRequest = {
		client: 'codex',
		machine: 'defaultHosted',
		directory: '/repo/project-a',
		runMode: 'foreground',
	};
	expect(inputJourneyPlanSchema.parse({ ...basePlan, pickerOnly: true, newRequest })).toMatchObject(
		{ pickerOnly: true, newRequest },
	);
	expect(() => inputJourneyPlanSchema.parse({ ...basePlan, pickerOnly: true })).toThrow();
	expect(() =>
		inputJourneyPlanSchema.parse({
			...basePlan,
			newRequest: { ...newRequest, machine: 'configured:Fixture remote' },
		}),
	).toThrow();
});
for (const keys of ['legacy', 'kitty'] as const) {
	for (const changed of [false, true]) {
		test(`${keys} Esc is observed through Help, compared, and exited only with Ctrl+C (changed=${changed})`, async () => {
			const root = mkdtempSync(join(tmpdir(), 's6-journey-'));
			roots.push(root);
			const output = join(root, 'evidence');
			mkdirSync(output);
			const bridge = controlledExecutable(
				root,
				'controlled-bridge',
				`
const { createInterface } = require('node:readline');
const fs = require('node:fs'); const path = require('node:path');
const receiptDirectory=process.env.TUI_SESSIONS_FIXTURE_RECEIPT_DIR;
const receiptPath=receiptDirectory && path.join(receiptDirectory,'input-dispatch.jsonl');
if(receiptPath)fs.writeFileSync(receiptPath,'');
let sequence=0;
const ack=(code,modifiers,surface,action)=>{if(receiptPath)fs.appendFileSync(receiptPath,JSON.stringify({sequence:++sequence,event:{kind:'key',code,modifiers,phase:'press'},surface,target:null,action})+'\\n');};
const emit = text => process.stdout.write('TUI_GRID ' + text + '\\n');
const frame = text => '\\x1b[?2026h\\x1b[2J\\x1b[H' + text + '\\x1b[?2026l';
const main = 'Codex Alpha';
const help = '\\x1b[2;2H┌────────────────────┐\\x1b[3;2H│ Help               │\\x1b[4;2H│ Surface Sessions   │\\x1b[5;2H│ Close              │\\x1b[6;2H└────────────────────┘';
let queue = [frame(main)], overlay = false, escaped = false, quit = false;
emit('READY 120 36');
createInterface({ input: process.stdin }).on('line', line => {
 if (line.startsWith('write ')) {
  const bytes = Buffer.from(line.slice(6), 'hex');
  const text = bytes.toString();
  if (text.includes('\\x1b\\x1bOP')) {emit('AMBIGUOUS_LEGACY_PREFIX');return;}
  if (text.includes('\\x03') || text.includes('[99;5u')) { quit = true; ack({kind:'shortcut',key:'c'},['control'],{kind:'screen'},'quit'); queue.push('SESSION_PICKER_OUTCOME canceled\\r\\n\\x1b[?1049l'); }
  else if (text.includes('OP') || text.includes('[1;1P')) { overlay = true; ack({kind:'fixed',name:'F1'},[],{kind:'screen'},'help'); queue.push(frame(help)); }
  else if (overlay) { overlay = false; ack({kind:'fixed',name:'Esc'},[],{kind:'overlay',overlayId:'1'},null); queue.push(frame(main + (${changed} && escaped ? ' changed query' : ''))); }
  else { escaped = true; ack({kind:'fixed',name:'Esc'},[],{kind:'screen'},'escape'); }
  emit('OK');
 } else if (line.startsWith('read ')) {
  const bytes = queue.shift();
  emit(bytes === undefined ? (quit ? 'EOF' : 'UNEXPECTED_READ_WITHOUT_FRAME') : 'BYTES ' + Buffer.from(bytes).toString('hex'));
 } else if (line === 'running') emit(quit ? 'EXITED' : 'RUNNING');
 else if (line === 'finish' || line === 'stop') { emit(line === 'finish' ? 'FINISHED 0' : 'STOPPED 0'); process.exit(0); }
 else emit('OK');
});
`,
			);
			const result = await observeMainEscape(
				baselinePlanSchema.parse({
					...basePlan,
					repository: root,
					output,
					driver: { ...basePlan.driver, path: bridge },
				}),
				keys,
				'sessions-filters',
			);
			expect(result.result).toBe(changed ? 'red' : 'pass');
			expect(result.observed).toMatchObject({ inputAcknowledgements: 4 });
			const commands: unknown = JSON.parse(
				readFileSync(
					join(output, `${keys}-sessions-filters-main-escape`, 'driver-commands.json'),
					'utf8',
				),
			);
			expect(commands).not.toContain('write 1b1b4f50');
			expect(commands).toContain(
				'write ' + Buffer.from(keys === 'legacy' ? '\x03' : '\x1b[99;5u').toString('hex'),
			);
		});
	}
}
