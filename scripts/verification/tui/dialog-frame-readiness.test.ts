/** Controlled bridge qualifies observer ordering, never product behavior. */
import { mkdirSync, mkdtempSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { expect, test } from 'vitest';

import { controlledExecutable } from './controlled-child-fixtures.ts';
import { openMeasuredDetails } from './dialog-frame-readiness.ts';
import { detailsActionCell } from './input-journeys.ts';
import { PtyGridDriver } from './pty-grid-driver.ts';
import type { KeyProtocol } from './pty-grid-driver.ts';

async function bridge(options: {
	readonly keys: KeyProtocol;
	readonly coalesced?: boolean;
	readonly missingRedraw?: boolean;
	readonly exit?: boolean;
	readonly badTarget?: boolean;
}): Promise<{ readonly driver: PtyGridDriver; readonly directory: string }> {
	const directory = mkdtempSync(join(tmpdir(), 'dialog-readiness-'));
	mkdirSync(join(directory, 'session-receipts'));
	const executable = controlledExecutable(
		directory,
		'controlled-bridge',
		`
const {createInterface}=require('node:readline');const fs=require('node:fs');const path=require('node:path');
const receipt=path.join(process.env.TUI_SESSIONS_FIXTURE_RECEIPT_DIR,'input-dispatch.jsonl');fs.writeFileSync(receipt,'');
const emit=t=>process.stdout.write('TUI_GRID '+t+'\\n');
const frame=t=>'\\x1b[?2026h\\x1b[2J\\x1b[H'+t+'\\x1b[?2026l';
const dialog=row=>'\\x1b[2;2H┌────────────────────────────────────────┐\\x1b[3;2H│ Session details                        │\\x1b['+row+';4H\\x1b[93m● Resume\\x1b[0m\\x1b[8;2H└────────────────────────────────────────┘';
let queue=[frame('Codex Alpha')];emit('READY 120 36');
createInterface({input:process.stdin}).on('line',line=>{
if(line.startsWith('write ')){
fs.appendFileSync(receipt,JSON.stringify({sequence:1,event:{kind:'key',code:{kind:'fixed',name:'Return'},modifiers:[],phase:'press'},surface:{kind:'screen'},target:${options.badTarget === true ? 'null' : "'sessions-list'"},action:'details'})+'\\n');
const draws=[frame('Codex Alpha pending main draw'),frame(dialog(6)),...(${options.missingRedraw === true}?[]:[frame(dialog(5))])];queue.push(...(${options.coalesced === true}?[draws.join('')]:draws));emit('OK');
}else if(line.startsWith('read ')){const next=queue.shift();emit(next===undefined?(${options.exit === true}?'EOF':${options.missingRedraw === true}?'READ_ERROR missing redraw':'BYTES 20'):'BYTES '+Buffer.from(next).toString('hex'));}
else if(line==='finish'||line==='stop'){emit(line==='finish'?'FINISHED 0':'STOPPED 0');process.exit(0);}
else emit('OK');});`,
	);
	const driver = await PtyGridDriver.start({
		repository: directory,
		driver: executable,
		surface: 'sessions-filters',
		keys: options.keys,
		evidence: directory,
		sessionsReceiptDirectory: join(directory, 'session-receipts'),
	});
	await driver.waitForGrid('main', () => true, 0);
	return { driver, directory };
}
for (const keys of ['legacy', 'kitty'] as const)
	for (const coalesced of [false, true]) {
		test(`${keys} targets measurement follow-up even with a pending main draw (coalesced=${coalesced})`, async () => {
			const { driver, directory } = await bridge({ keys, coalesced });
			try {
				await openMeasuredDetails(driver, directory, keys);
				expect(detailsActionCell(driver.grid(), 'codex', 'resume')).toEqual({ column: 5, row: 4 });
				expect(
					JSON.parse(readFileSync(join(directory, 'details-frame-binding.json'), 'utf8')),
				).toMatchObject({
					beforeFrame: 1,
					measurementFrame: 3,
					minimumCompletedFrame: 4,
					receivedCompletedFrame: 4,
					inputAcknowledgement: { sequence: 1, action: 'details' },
				});
			} finally {
				await driver.stop();
			}
		});
	}
test('a title-bearing first paint without its measurement redraw cannot supply a target', async () => {
	const { driver, directory } = await bridge({ keys: 'legacy', missingRedraw: true });
	try {
		await expect(openMeasuredDetails(driver, directory, 'legacy')).rejects.toThrow(
			'missing redraw',
		);
	} finally {
		await driver.stop();
	}
});
test('exit drain requires EOF followed by successful driver finish', async () => {
	const { driver } = await bridge({ keys: 'legacy', exit: true });
	try {
		expect(await driver.drainUntilExit()).toBe(0);
	} finally {
		await driver.stop();
	}
});
test('a screen-level null target cannot stand in for the focused list activation', async () => {
	const { driver, directory } = await bridge({ keys: 'legacy', badTarget: true });
	try {
		await expect(openMeasuredDetails(driver, directory, 'legacy')).rejects.toThrow(
			'target mismatch',
		);
	} finally {
		await driver.stop();
	}
});
test('an indefinitely live child producing bytes cannot escape the apparatus exit deadline', async () => {
	const { driver, directory } = await bridge({ keys: 'legacy' });
	try {
		await expect(driver.drainUntilExit(30)).rejects.toThrow('apparatus exit deadline');
	} finally {
		await driver.stop();
	}
	expect(JSON.parse(readFileSync(join(directory, 'driver-commands.json'), 'utf8'))).not.toContain(
		'finish',
	);
});
