/** Controlled transport qualifies the real legend observer; this is not product evidence. */
import { mkdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

import { expect, test } from 'vitest';

import { baselinePlanSchema, observeIsolation } from './baseline-journeys.ts';
import { controlledExecutable } from './controlled-child-fixtures.ts';
import { temporaryRoot } from './receipt-fixtures.ts';

for (const keys of ['legacy', 'kitty'] as const)
	for (const missing of [false, true]) {
		test(`${keys} Help reaches the named convention with semantic End (missing=${missing})`, async () => {
			const root = temporaryRoot(),
				output = join(root, 'evidence');
			mkdirSync(output);
			const bridge = controlledExecutable(
				root,
				'legend-bridge',
				`
const {createInterface}=require('node:readline');
const emit=text=>process.stdout.write('TUI_GRID '+text+'\\n');
const frame=text=>'\\x1b[?2026h\\x1b[2J\\x1b[H'+text+'\\x1b[?2026l';
const main='\\x1b[38;2;255;255;255mCodex Alpha';
const covered='\\x1b[38;2;100;100;100mCodex Alpha';
const help=legend=>covered+'\\x1b[2;2H┌────────────────────────────────────────────────────────────────────────────┐\\x1b[3;2H│ Help                                                                       │\\x1b[4;2H│ Surface Sessions                                                           │\\x1b[5;2H│ '+(legend?'Esc clear filter, reset filters, never quit':'Generated actions fill the first Help viewport')+'\\x1b[6;2H└────────────────────────────────────────────────────────────────────────────┘';
let queue=[frame(main),frame(main)],overlay=false;
emit('READY 120 36');
createInterface({input:process.stdin}).on('line',line=>{
 if(line.startsWith('write ')){
  const text=Buffer.from(line.slice(6),'hex').toString();
  if(text.includes('OP')||text.includes('[1;1P')) {overlay=true;queue.push(frame(help(false)));}
  else if(overlay&&(text.includes('[F')||text.includes('[1;1F')))queue.push(frame(help(!${missing})));
  else if(overlay&&text.includes('[B'))queue.push(frame(help(false)));
  else if(overlay){overlay=false;queue.push(frame(main));}
  emit('OK');
 }else if(line.startsWith('read ')) {const next=queue.shift();emit(next===undefined?'UNEXPECTED_READ_WITHOUT_FRAME':'BYTES '+Buffer.from(next).toString('hex'));}
 else if(line==='stop'){emit('STOPPED 0');process.exit(0);}
 else emit('OK');
});
`,
			);
			const plan = baselinePlanSchema.parse({
				repository: root,
				productSourceHead: 'controlled',
				driver: { path: bridge, sha256: 'a'.repeat(64) },
				product: { path: '/not-launched', sha256: 'b'.repeat(64) },
				output,
				pickerOnly: true,
			});
			const observed = observeIsolation(
				plan,
				keys,
				'sessions-filters',
				'Help',
				Buffer.from(keys === 'legacy' ? '\x1bOP' : '\x1b[1;1P'),
			);
			if (missing) {
				await expect(observed).rejects.toThrow();
				return;
			}
			const results = await observed;
			const legendResult = results.find(
				(row) => row.name === 'Help describes Esc as never quitting',
			);
			expect(legendResult?.result).toBe('pass');
			expect(JSON.stringify(legendResult?.observed)).toContain('never quit');
			const commands: unknown = JSON.parse(
				readFileSync(
					join(output, keys + '-sessions-filters-isolation-Help', 'driver-commands.json'),
					'utf8',
				),
			);
			expect(commands).toContain(
				'write ' + Buffer.from(keys === 'legacy' ? '\x1b[F' : '\x1b[1;1F').toString('hex'),
			);
			expect(commands).not.toContain('write ' + Buffer.from('\x1b[B'.repeat(8)).toString('hex'));
		});
	}
