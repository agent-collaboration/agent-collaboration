/** Render original PTY bytes with the real wterm DOM renderer, then capture Chrome's PNG.
 * node scripts/verification/tui/terminal/terminal-original.ts --help
 */
import { createHash } from 'node:crypto';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { z } from 'zod';

import { isMain, reportCliFailure } from '../cli/cli-arguments.ts';
import { ChromeDevtools } from './chrome-devtools.ts';
import type { TerminalGrid, TerminalResize } from './terminal-grid.ts';
import {
	createTerminalCore,
	ghosttyWasmBytes,
	gridLines,
	snapshotGrid,
	replayTerminal,
	terminalTheme,
} from './terminal-grid.ts';

export interface OriginalCapture {
	readonly chrome: string;
	readonly output: string;
	readonly bytes: Uint8Array;
	readonly columns: number;
	readonly rows: number;
	readonly checkpoint: string;
	readonly sourceHead: string;
	readonly binarySha256: string;
	readonly grid?: TerminalGrid;
	readonly resizes?: readonly TerminalResize[];
}
const evaluationSchema = z.object({
	result: z.object({ value: z.unknown().optional() }),
	exceptionDetails: z.unknown().optional(),
});
const hash = (bytes: Uint8Array): string => createHash('sha256').update(bytes).digest('hex');

export async function renderOriginal(options: OriginalCapture): Promise<unknown> {
	const packageRoots = Object.fromEntries(
		['core', 'dom', 'ghostty'].map((name) => [
			name,
			dirname(dirname(fileURLToPath(import.meta.resolve(`@wterm/${name}`)))),
		]),
	);
	const html = `<!doctype html><meta charset="utf-8"><link rel="stylesheet" href="/dom/src/terminal.css">
<style>html,body{margin:0;background:#1e1e1e}#terminal{--term-font-size:16px;--term-line-height:20px;font:16px Menlo,monospace;line-height:20px;width:${options.columns}ch;height:${options.rows * 20}px;padding:0;border-radius:0;box-shadow:none}</style>
<script type="importmap">{"imports":{"@wterm/core":"/core/dist/index.js"}}</script><div id="terminal"></div>`;
	const server = createServer((request, response) => {
		const path = request.url ?? '/';
		if (path === '/') {
			response.setHeader('Content-Type', 'text/html');
			response.end(html);
			return;
		}
		const match = /^\/(core|dom|ghostty)\/([a-z0-9./_-]+)$/iu.exec(path);
		const root = match?.[1] === undefined ? undefined : packageRoots[match[1]];
		const relative = match?.[2];
		if (root === undefined || relative === undefined || relative.split('/').includes('..')) {
			response.writeHead(404);
			response.end();
			return;
		}
		const asset = join(root, relative);
		if (!existsSync(asset)) {
			response.writeHead(404);
			response.end();
			return;
		}
		response.setHeader(
			'Content-Type',
			asset.endsWith('.wasm')
				? 'application/wasm'
				: asset.endsWith('.css')
					? 'text/css'
					: 'text/javascript',
		);
		response.end(readFileSync(asset));
	});
	await new Promise<void>((resolve, reject) => {
		server.once('error', reject);
		server.listen(0, '127.0.0.1', resolve);
	});
	const address = server.address();
	if (address === null || typeof address === 'string') throw new Error('asset listener missing');
	const chrome = new ChromeDevtools(options.chrome, options.output);
	try {
		const target = z
			.object({ targetId: z.string() })
			.parse(await chrome.send('Target.createTarget', { url: 'about:blank' }));
		const session = z
			.object({ sessionId: z.string() })
			.parse(
				await chrome.send('Target.attachToTarget', { targetId: target.targetId, flatten: true }),
			).sessionId;
		await chrome.send('Page.enable', {}, session);
		const loaded = chrome.waitForEvent('Page.loadEventFired', session);
		await Promise.all([
			loaded,
			chrome.send('Page.navigate', { url: `http://127.0.0.1:${address.port}/` }, session),
		]);
		const payload = Buffer.from(options.bytes).toString('base64');
		const resizes = options.resizes ?? [];
		const initialColumns = resizes.length === 0 ? options.columns : 120;
		const initialRows = resizes.length === 0 ? options.rows : 36;
		// Parsing is synchronous in GhosttyCore. Two animation frames drain
		// WTerm's scheduled paint; no time-based screenshot delay is used.
		const script = `(async()=>{
 const {WTerm}=await import('/dom/dist/index.js');
 const {GhosttyCore}=await import('/ghostty/dist/index.js');
 await document.fonts.ready;
 const core=await GhosttyCore.load({wasmPath:'/ghostty/wasm/ghostty-vt.wasm',imageStorageLimit:0});
 const terminal=new WTerm(document.getElementById('terminal'),{core,cols:${initialColumns},rows:${initialRows},autoResize:false,cursorBlink:false});
 await terminal.init(); terminal.setThemeColors(${JSON.stringify(terminalTheme)}); const bytes=Uint8Array.from(atob(${JSON.stringify(payload)}),c=>c.charCodeAt(0));let offset=0;for(const resize of ${JSON.stringify(resizes)}){terminal.write(bytes.subarray(offset,resize.byteOffset));terminal.resize(resize.columns,resize.rows);offset=resize.byteOffset;}terminal.write(bytes.subarray(offset));
 const rowHeight=Number.parseFloat(getComputedStyle(terminal.element).getPropertyValue('--term-row-height'));
 if(!Number.isFinite(rowHeight)||rowHeight<=0)throw new Error('renderer row height is invalid');
 terminal.element.style.height=(core.getRows()*rowHeight)+'px';terminal.element.scrollTop=0;
 await new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)));
 const rect=terminal.element.getBoundingClientRect();
 if(Math.abs(rect.height-core.getRows()*rowHeight)>0.01)throw new Error('renderer height differs from its terminal rows');
 const lines=Array.from({length:core.getRows()},(_,row)=>Array.from({length:core.getCols()},(_,col)=>{const cell=core.getCell(row,col);return cell.width===0||cell.spacerHead?'':cell.chars??String.fromCodePoint(cell.char)}).join(''));
 return {lines,columns:core.getCols(),rows:core.getRows(),width:rect.width,height:rect.height};
})()`;
		const evaluated = evaluationSchema.parse(
			await chrome.send(
				'Runtime.evaluate',
				{
					expression: script,
					awaitPromise: true,
					returnByValue: true,
				},
				session,
			),
		);
		if (evaluated.exceptionDetails !== undefined)
			throw new Error(`renderer failed: ${JSON.stringify(evaluated.exceptionDetails)}`);
		const rendered = z
			.object({
				lines: z.array(z.string()),
				columns: z.number(),
				rows: z.number(),
				width: z.number().positive(),
				height: z.number().positive(),
			})
			.parse(evaluated.result.value);
		if (rendered.columns !== options.columns || rendered.rows !== options.rows)
			throw new Error('browser grid mismatch');
		const headless = await createTerminalCore(initialColumns, initialRows);
		try {
			replayTerminal(headless, options.bytes, resizes);
			if (
				JSON.stringify(gridLines(snapshotGrid(headless))) !== JSON.stringify(rendered.lines) ||
				(options.grid !== undefined &&
					JSON.stringify(gridLines(options.grid)) !== JSON.stringify(rendered.lines))
			)
				throw new Error('browser original content differs from the headless observation');
		} finally {
			headless.dispose();
		}
		const image = z.object({ data: z.string() }).parse(
			await chrome.send(
				'Page.captureScreenshot',
				{
					format: 'png',
					captureBeyondViewport: true,
					clip: { ['x']: 0, ['y']: 0, width: rendered.width, height: rendered.height, scale: 1 },
				},
				session,
			),
		);
		const png = Buffer.from(image.data, 'base64');
		writeFileSync(join(options.output, `${options.checkpoint}.png`), png);
		writeFileSync(join(options.output, `${options.checkpoint}.ansi`), options.bytes);
		const receipt = {
			kind: 'original-wterm-dom-png',
			sourceHead: options.sourceHead,
			binarySha256: options.binarySha256,
			checkpoint: options.checkpoint,
			viewport: { columns: options.columns, rows: options.rows },
			ptySha256: hash(options.bytes),
			resizes,
			pngSha256: hash(png),
			rendered,
			chromeBinarySha256: hash(readFileSync(options.chrome)),
			renderer: '@wterm/dom0.5.7 + @wterm/ghostty0.5.7',
			physicalTerminal: false,
			node: process.version,
			wasmSha256: hash(ghosttyWasmBytes()),
			rendererSourceSha256: hash(readFileSync(fileURLToPath(import.meta.url))),
		};
		writeFileSync(
			join(options.output, `${options.checkpoint}.original.json`),
			`${JSON.stringify(receipt, null, 2)}\n`,
		);
		return receipt;
	} finally {
		try {
			await chrome.close();
		} finally {
			await new Promise<void>((resolve, reject) =>
				server.close((error) => (error === undefined ? resolve() : reject(error))),
			);
		}
	}
}
if (isMain(import.meta.url)) {
	if (process.argv[2] === '--help')
		process.stdout.write(
			'terminal-original.ts is the wterm DOM/DevTools image sink; use the declared grid capture runner. No reconstructed cell image is accepted.\n',
		);
	else reportCliFailure(new Error('use the declared grid capture runner'));
}
