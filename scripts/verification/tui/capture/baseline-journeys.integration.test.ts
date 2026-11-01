/** Observer apparatus only: controlled PTY protocol bytes are never product evidence. */
import { mkdirSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { afterEach, expect, test, vi } from 'vitest';

import { controlledExecutable } from '../fixtures/controlled-child-fixtures.ts';
import { PtyGridDriver } from '../terminal/pty-grid-driver.ts';
import { baselinePlanSchema, observeMainEscape } from './baseline-journeys.ts';
import { inputJourneyPlanSchema } from './input-journeys.ts';

// Writer completion must suffice when every filesystem notification is lost.
vi.mock('node:fs', async (importOriginal) => {
	const original = await importOriginal<typeof import('node:fs')>();
	return {
		...original,
		watch: (path: string, options: { persistent: boolean }) =>
			original.watch(path, options, () => {}),
	};
});

const roots: string[] = [];
afterEach(() => {
	vi.restoreAllMocks();
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
	for (const coalesced of [false, true]) {
		for (const { changed, badTarget, completionFailure } of [
			{ changed: false, badTarget: false, completionFailure: false },
			{ changed: true, badTarget: false, completionFailure: false },
			{ changed: false, badTarget: true, completionFailure: false },
			{ changed: false, badTarget: false, completionFailure: true },
		]) {
			test(`${keys} Esc is observed through Help, compared, and exited only with Ctrl+C (changed=${changed}, badTarget=${badTarget}, completionFailure=${completionFailure}, coalesced=${coalesced})`, async () => {
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
const ack=(code,modifiers,surface,action,target=null)=>{if(receiptPath)fs.appendFileSync(receiptPath,JSON.stringify({sequence:++sequence,event:{kind:'key',code,modifiers,phase:'press'},surface,target,action})+'\\n');};
const emit = text => process.stdout.write('TUI_GRID ' + text + '\\n');
const frame = text => '\\x1b[?2026h\\x1b[2J\\x1b[H' + text + '\\x1b[?2026l';
const main = 'Codex Alpha';const settled=main+' ❯';
const help = '\\x1b[2;2H┌────────────────────┐\\x1b[3;2H│ Help               │\\x1b[4;2H│ Surface Sessions   │\\x1b[5;2H│ Close              │\\x1b[6;2H└────────────────────┘';
let queue = ${coalesced}?[frame(main)+frame(settled)]:[frame(main),frame(settled)], overlay = false, escaped = false, quit = false;
const flush=()=>{for(const bytes of queue)emit('BYTES '+Buffer.from(bytes).toString('hex'));queue=[];};emit('READY 120 36 STREAM');flush();
createInterface({ input: process.stdin }).on('line', line => {
 if (line.startsWith('write ')) {
  const bytes = Buffer.from(line.slice(6), 'hex');
  const text = bytes.toString();
  if (text.includes('\\x1b\\x1bOP')) {emit('AMBIGUOUS_LEGACY_PREFIX');return;}
  if (text.includes('\\x03') || text.includes('[99;5u')) { quit = true; ack({kind:'shortcut',key:'c'},['control'],{kind:'screen'},'quit'); queue.push('SESSION_PICKER_OUTCOME canceled\\r\\n\\x1b[?1049l'); }
  else if (text.includes('OP') || text.includes('[1;1P')) { overlay = true; ack({kind:'fixed',name:'F1'},[],{kind:'screen'},'help'); queue.push(frame(help)); }
  else if (overlay) { overlay = false; ack({kind:'fixed',name:'Esc'},[],{kind:'overlay',overlayId:'1'},null,${badTarget}?null:Buffer.byteLength('action:help')+':action:help:help-body'); queue.push(frame(settled + (${changed} && escaped ? ' changed query' : ''))); }
  else { escaped = true; ack({kind:'fixed',name:'Esc'},[],{kind:'screen'},'escape'); queue.push(frame(settled)); }
  if(!quit)queue.push('\\x1b]7770;dispatch='+sequence+'\\x1b\\\\');emit('OK');flush();if(quit)emit('EOF');
 } else if (line.startsWith('read ')) {emit('OK');
 } else if (line === 'running') emit(quit ? 'EXITED' : 'RUNNING');
 else if (line === 'finish' || line === 'stop') { emit(line === 'finish' ? 'FINISHED 0' : 'STOPPED 0'); process.exit(0); }
 else emit('OK');
});
`,
				);
				if (completionFailure)
					vi.spyOn(PtyGridDriver.prototype, 'waitForDispatchFrame').mockRejectedValueOnce(
						new Error('controlled frame completion failed'),
					);
				const observation = observeMainEscape(
					baselinePlanSchema.parse({
						...basePlan,
						pickerOnly: true,
						repository: root,
						output,
						driver: { ...basePlan.driver, path: bridge },
					}),
					keys,
					'sessions-filters',
				);
				if (completionFailure) {
					await expect(observation).rejects.toThrow('controlled frame completion failed');
					const journal = readFileSync(
						join(
							output,
							keys + '-sessions-filters-main-escape',
							'session-receipts',
							'input-dispatch.jsonl',
						),
						'utf8',
					);
					expect(JSON.parse(journal)).toMatchObject({ sequence: 1, action: 'escape' });
					return;
				}
				if (badTarget) {
					await expect(observation).rejects.toThrow('target mismatch');
					return;
				}
				const result = await observation;
				expect(result.result).toBe(changed ? 'red' : 'pass');
				expect(result.observed).toMatchObject({ inputAcknowledgements: 4 });
				const entryBinding: unknown = JSON.parse(
					readFileSync(
						join(output, `${keys}-sessions-filters-main-escape`, 'entry-frame-binding.json'),
						'utf8',
					),
				);
				expect(entryBinding).toMatchObject({ minimumCompletedFrame: 2, receivedCompletedFrame: 2 });
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
}
