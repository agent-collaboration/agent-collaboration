/** Native ContentScroll and Help aliases against a physical one-row census and fixed account order. */
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';

import type { ProcessIdentity } from '../../evidence/evidence-schema.ts';
import {
	lookupProcessIdentity,
	requireOwnedProductChild,
	sameIdentity,
} from '../../processes/process-identity.ts';
import type { InputConsumptionExpectation } from '../../sessions/input-consumption-schema.ts';
import type { MarkedInputEvidence } from '../../terminal/marker-input-session.ts';
import { gridLines, terminalTheme } from '../../terminal/terminal-grid.ts';
import type { TerminalGrid } from '../../terminal/terminal-grid.ts';
import { renderOriginal } from '../../terminal/terminal-original.ts';
import { journeyGridSizes } from '../journey-definition.ts';
import { findDialog } from '../scenes/grid-scene-matcher.ts';
import {
	helpViewport,
	paintedHelpContentRows,
	requireHelpNoop,
	requireHelpRows,
} from '../scenes/option-navigation-observation.ts';
import type { AuthoredFlowPlan } from './authored-flow-plan.ts';
import type { FlowCheck } from './flow-definition.ts';
import type { FlowInvocation } from './flow-executor.ts';
import type {
	NativeFlowBindings,
	NativeFlowContext,
	NativeFlowInputBinding,
} from './native-flow-runtime.ts';
import { locatePaintedTarget } from './painted-flow-target.ts';
import { collectQuotaContent, quotaContentViewport } from './quota-content-census.ts';
import {
	collectQuotaDialog,
	quotaExpectedKey,
	quotaMarkedFrame,
	quotaOverlay,
	quotaReadOnlyBaseBindings,
	quotaScreen,
} from './quota-flow-inputs.ts';
import { checkQuotaHelpRows } from './quota-toolbar-observation.ts';
const aliases = ['Alt+Shift+Up', 'Alt+Down', 'Alt+Up', 'Alt+Shift+Down'] as const;
const helpBody = '17:action:quota:help:help-body';
type PhysicalOracle =
	| { readonly kind: 'overflow'; readonly full: readonly string[]; readonly height: number }
	| { readonly kind: 'fit'; readonly grid: TerminalGrid };
export function requireQuotaNavigationLabels(full: readonly string[]): void {
	const actual = [...full.join('\n').matchAll(/\b[0-9]{2} Account/gu)].map((match) => match[0]);
	const expected = Array.from(
		{ length: 60 },
		(_, index) => String(index).padStart(2, '0') + ' Account',
	);
	if (JSON.stringify(actual) !== JSON.stringify(expected))
		throw new Error('quota content census differs from the fixed sixty-account order');
}
function quotaNavigationFirstFocused(grid: TerminalGrid): boolean {
	try {
		locatePaintedTarget(grid, {
			label: '00 Account',
			bounds: { left: 0, right: grid.columns, top: 2, bottom: grid.rows },
			foregrounds: [terminalTheme.palette[11] ?? 0xd7ba7d],
		});
		return true;
	} catch {
		return false;
	}
}
export function quotaOptionBindings(
	plan: AuthoredFlowPlan,
	invocation: FlowInvocation,
	evidence: string,
): NativeFlowBindings | null {
	const name = invocation.journey.journeyId;
	if (name !== 'quota-list-option-navigation' && name !== 'quota-help-option-navigation')
		return null;
	const help = name === 'quota-help-option-navigation',
		base = quotaReadOnlyBaseBindings(plan, invocation, evidence, 'large-list');
	const surface = help ? quotaOverlay(1) : quotaScreen,
		target = help ? helpBody : 'action:quota:accounts';
	const expected = (key: string): InputConsumptionExpectation =>
		quotaExpectedKey(key, surface, target);

	const inputs = new Map<number, NativeFlowInputBinding>();
	for (const [index, step] of invocation.journey.steps.entries()) {
		if (step.kind === 'press')
			inputs.set(index, {
				kind: 'continuing',
				expected: [
					step.key === 'F1'
						? quotaExpectedKey('F1', quotaScreen, null, 'quota:help')
						: expected(step.key),
				],
			});
		if (step.kind === 'focus')
			inputs.set(index, {
				kind: 'focus',
				stopWhenFocused: true,
				route: Array.from({ length: 12 }, () => ({
					key: 'Tab',
					expected: [quotaExpectedKey('Tab', quotaScreen)],
				})),
				isFocused: quotaNavigationFirstFocused,
			});
	}
	let child: ProcessIdentity | null = null,
		oracle: PhysicalOracle | null = null,
		ordinal = 0,
		nextDetails = 1,
		censusOrdinal = 0;
	const checkpoints = invocation.journey.steps.flatMap((step) =>
		step.kind === 'capture' &&
		step.checks.includes(help ? 'help-option-page-and-ends' : 'content-scroll-option-page-and-ends')
			? [step.checkpoint]
			: [],
	);
	const requireChild = (): void => {
		if (child === null || !sameIdentity(lookupProcessIdentity(child.pid), child))
			throw new Error('quota navigation lost exact product identity');
	};
	const qualify = async (context: NativeFlowContext): Promise<MarkedInputEvidence> => {
		let marked = await context.input.press('Alt+Shift+Up', [expected('Alt+Shift+Up')]);
		const first = quotaMarkedFrame(marked);
		if (help) {
			const result = await collectQuotaDialog({
				context: { ...context, frame: first },
				title: 'Help',
				surface,
				focusedTarget: helpBody,
			});
			marked = result.marked;
			const failures = checkQuotaHelpRows(result.rows);
			if (failures.length > 0) throw new Error(failures.join('; '));
			const rectangle = findDialog(first.grid, 'Help');
			if (rectangle === null) throw new Error('quota Help absent');
			let viewport;
			try {
				viewport = helpViewport(first.grid);
			} catch {
				viewport = null;
			}
			oracle =
				viewport === null
					? { kind: 'fit', grid: first.grid }
					: {
							kind: 'overflow',
							full: paintedHelpContentRows(result.rows),
							height: viewport.rows.length,
						};
		} else {
			const result = await collectQuotaContent({ ...context, frame: first });
			marked = result.marked;
			requireQuotaNavigationLabels(result.full);
			if (result.full.length <= result.height * 2)
				throw new Error('quota seed does not exceed two physical pages');
			oracle = {
				kind: 'overflow',
				full: paintedHelpContentRows(result.full),
				height: result.height,
			};
		}
		writeFileSync(
			join(evidence, 'quota-physical-census-' + ++censusOrdinal + '.json'),
			JSON.stringify({ child, oracle, entry: first.binding }, null, 2),
		);
		return marked;
	};
	const destination = (grid: TerminalGrid, checkpoint: string): void => {
		requireChild();
		if (oracle === null) throw new Error('quota physical oracle absent');
		if (oracle.kind === 'fit') {
			requireHelpNoop(oracle.grid, grid);
			return;
		}
		const maximum = Math.max(0, oracle.full.length - oracle.height),
			offset =
				checkpoint === 'alias-last'
					? maximum
					: checkpoint === 'alias-page-down'
						? Math.min(maximum, oracle.height)
						: 0;
		const viewport = help ? helpViewport(grid) : quotaContentViewport(grid);
		requireHelpRows(viewport.rows, oracle.full.slice(offset, offset + oracle.height));
		if (checkpoint === 'alias-last' && !viewport.atEnd)
			throw new Error('quota Last thumb does not reach painted content end');
	};
	const verifySelected = async (context: NativeFlowContext): Promise<MarkedInputEvidence> => {
		const details = quotaOverlay(nextDetails++);
		const marked = await context.input.press('F4', [
			quotaExpectedKey('F4', quotaScreen, null, 'quota:details'),
		]);
		const grid = quotaMarkedFrame(marked).grid,
			rectangle = findDialog(grid, 'Account details');
		if (rectangle === null) throw new Error('selection probe Details absent');
		const body = gridLines(grid)
			.slice(rectangle.top + 1, rectangle.bottom)
			.join('')
			.replace(/\s/gu, '');
		if (!body.includes('00Account')) throw new Error('ContentScroll changed the selected account');
		return context.input.press('Escape', [
			quotaExpectedKey('Escape', details, 'action:quota:account-actions'),
		]);
	};
	const fixtures = new Map<
		string,
		(context: NativeFlowContext) => Promise<MarkedInputEvidence | null>
	>([
		[
			'seed-option-navigation-' + (help ? 'quota-help' : 'quota-list'),
			async (context) => {
				child = requireOwnedProductChild(context.driver.processId(), plan.product.path);
				if (!quotaNavigationFirstFocused(context.frame.grid))
					throw new Error('large-list did not start focused on its first literal account');
				writeFileSync(
					join(evidence, 'option-child-binding.json'),
					JSON.stringify({ child, product: plan.product }, null, 2),
				);
				return help ? null : qualify(context);
			},
		],
		['qualify-native-quota-help-physical-rows', qualify],
		['verify-quota-selected-after-scroll', verifySelected],
		[
			'try-consumed-original-navigation-keys',
			async (context) => {
				const start = context.input.input.history().length;
				await ['PageUp', 'PageDown', 'Home', 'End'].reduce(async (previous, key) => {
					await previous;
					if ((await context.input.input.key(key)).delivery.kind !== 'consumed')
						throw new Error('original navigation key was not consumed by the declared profile');
				}, Promise.resolve());
				writeFileSync(
					join(evidence, 'consumed-original-keys.json'),
					JSON.stringify(context.input.input.history().slice(start), null, 2),
				);
				return null;
			},
		],
		[
			'resize-same-child-to-each-declared-grid',
			async (context) => {
				let marked: MarkedInputEvidence | null = null;
				await journeyGridSizes.reduce(async (previous, dimensions) => {
					await previous;
					requireChild();
					const [columns, rows] = dimensions.split('x').map(Number);
					if (columns === undefined || rows === undefined)
						throw new Error('quota resize dimensions absent');
					const frame = marked === null ? context.frame : quotaMarkedFrame(marked);
					marked =
						frame.grid.columns === columns && frame.grid.rows === rows
							? await context.input.press('F12', [quotaExpectedKey('F12', surface)])
							: await context.input.resize(columns, rows, [
									{ event: { kind: 'resize', columns, rows }, surface, target: null, action: null },
								]);
					if (context.driver.inputModes().kittyFlags !== (invocation.keys === 'legacy' ? 0 : 2))
						throw new Error('quota resize changed keyboard modes');
					marked = await qualify({ ...context, frame: quotaMarkedFrame(marked) });
					await aliases.reduce(async (prior, key) => {
						await prior;
						marked = await context.input.press(key, [expected(key)]);
						destination(
							quotaMarkedFrame(marked).grid,
							key === 'Alt+Down'
								? 'alias-page-down'
								: key === 'Alt+Shift+Down'
									? 'alias-last'
									: 'alias-first',
						);
					}, Promise.resolve());
					if (marked === null) throw new Error('quota resize has no frame');
					const final = quotaMarkedFrame(marked);
					await renderOriginal({
						...base.original,
						output: evidence,
						checkpoint: 'quota-option-resize-' + dimensions,
						columns,
						rows,
						bytes: final.bytes,
						grid: final.grid,
						resizes: final.resizes,
					});
					if (!help) marked = await verifySelected({ ...context, frame: final });
					requireChild();
				}, Promise.resolve());
				return marked;
			},
		],
	]);
	const checkName: FlowCheck = help
		? 'help-option-page-and-ends'
		: 'content-scroll-option-page-and-ends';
	return {
		...base,
		inputs,
		fixtures,
		targets: new Map([
			[
				help ? 'main list before Help' : 'main list',
				(grid) => ({
					label: '00 Account',
					bounds: { left: 0, right: grid.columns, top: 2, bottom: grid.rows },
				}),
			],
		]),
		checks: new Map([
			[
				checkName,
				async (context: NativeFlowContext) => {
					const checkpoint = checkpoints[ordinal++];
					if (checkpoint === undefined) throw new Error('quota navigation checkpoint absent');
					if (checkpoint === 'alias-help-return') {
						requireChild();
						if (
							findDialog(context.frame.grid, 'Help') !== null ||
							!quotaNavigationFirstFocused(context.frame.grid)
						)
							return ['Help lost first selected account/focus on return'];
					} else destination(context.frame.grid, checkpoint);
					return [];
				},
			],
			[
				'option-alias-delivery',
				async (context: NativeFlowContext) => {
					const record = context.input.input.history().at(-1);
					if (record?.input.kind !== 'key' || record.delivery.kind !== 'sent')
						return ['quota alias was not delivered'];
					const inputKey = record.input.key;
					if (!aliases.some((key) => key === inputKey)) return ['unexpected quota alias'];
					const modifier = record.input.key.includes('Shift') ? 4 : 3,
						final = record.input.key.endsWith('Up') ? 'A' : 'B';
					const bytes = '\x1b[1;' + modifier + (invocation.keys === 'kitty' ? ':1' : '') + final;
					return record.delivery.hex === Buffer.from(bytes).toString('hex')
						? []
						: ['quota alias bytes lost modifiers/press suffix'];
				},
			],
		]),
	};
}
