/** Own the single control connection and independent journal for one real quota invocation. */
import { readFileSync } from 'node:fs';

import { z } from 'zod';

import {
	checkQuotaEffectReceipt,
	quotaAccountFactSchema,
	quotaCapturedTargetSchema,
	quotaEffectReceiptSchema,
	quotaPoseSchema,
	quotaRefreshReceiptSchema,
} from './quota-effect-readback.ts';
import { QuotaEventJournal, quotaEventRecordSchema } from './quota-event-journal.ts';
import { quotaEffectScenarioSchema } from './quota-fixture-contract.ts';
import type { QuotaFixtureEventKind } from './quota-fixture-control.ts';
import { QuotaFixtureControl } from './quota-fixture-control.ts';
import type { QuotaFixtureRequest, QuotaFixtureResponse } from './quota-fixture-control.ts';
import type { QuotaFixtureLayout } from './quota-fixture-layout.ts';
import { observeQuotaEvent } from './quota-handoff-bindings.ts';

export const quotaLivePoseSchema = quotaPoseSchema.extend({
	rows: z.array(
		quotaPoseSchema.shape.rows.element.extend({
			label: z.string(),
			provider: z.string(),
			status: z.string(),
			enabled: z.boolean(),
			weeklyWindow: z.string(),
			detailsWeeklyWindow: z.string(),
			resetCredits: z.string(),
		}),
	),
	scope: z.string(),
	focus: z.string().nullable(),
	actions: z.array(
		z.strictObject({
			['id']: z.string(),
			title: z.string(),
			keys: z.array(z.string()),
			available: z.boolean(),
			availability: z.string(),
			checked: z.boolean().nullable(),
		}),
	),
	published: z.unknown(),
});
export const quotaLiveReadbackSchema = z.strictObject({
	kind: z.literal('fixture-readback'),
	requestId: z.number().int().positive(),
	readback: quotaLivePoseSchema,
	accounts: z.array(quotaAccountFactSchema),
	accountCommands: quotaEffectReceiptSchema.shape.accountCommands,
	refresh: quotaRefreshReceiptSchema.nullable(),
});
export type QuotaLiveReadback = z.infer<typeof quotaLiveReadbackSchema>;
export const quotaObservationSchema = z.strictObject({
	live: quotaLiveReadbackSchema,
	events: z.array(quotaEventRecordSchema),
	routerRoot: z.string().startsWith('/'),
});
const startedSchema = z.strictObject({
	kind: z.literal('fixture-started'),
	scenario: quotaEffectScenarioSchema,
	routerRoot: z.string().startsWith('/'),
	capturedTarget: quotaCapturedTargetSchema,
	accounts: z.array(quotaAccountFactSchema),
});
interface QuotaFixtureAccess {
	readonly control: QuotaFixtureControl;
	readonly journal: QuotaEventJournal;
	readonly started: z.infer<typeof startedSchema>;
}

export class QuotaLiveFixture {
	private readonly layout: QuotaFixtureLayout;
	private opening: Promise<QuotaFixtureAccess> | null = null;
	private opened: QuotaFixtureAccess | null = null;
	private cached: { readonly sequence: number; readonly value: Promise<QuotaLiveReadback> } | null =
		null;
	constructor(layout: QuotaFixtureLayout) {
		this.layout = layout;
	}
	access(): Promise<QuotaFixtureAccess> {
		this.opening ??= this.open();
		return this.opening;
	}
	private async open(): Promise<QuotaFixtureAccess> {
		const control = await QuotaFixtureControl.connect(this.layout.configuration.controlSocket);
		try {
			const journal = new QuotaEventJournal(this.layout.configuration.events);
			const first = journal.read().find((record) => record.event.kind === 'fixture-started');
			const started = startedSchema.parse(first?.event);
			if (started.scenario !== this.layout.configuration.scenario)
				throw new Error('quota started scenario differs from declared launch');
			this.opened = { control, journal, started };
			return this.opened;
		} catch (error) {
			control.close();
			throw error;
		}
	}
	/** Concurrent checks of one marked frame share one actual request, never concurrent socket writes. */
	read(sequence: number): Promise<QuotaLiveReadback> {
		if (this.cached?.sequence !== sequence)
			this.cached = {
				sequence,
				value: this.access().then(async ({ control }) =>
					quotaLiveReadbackSchema.parse(await control.request({ operation: 'readback' })),
				),
			};
		return this.cached.value;
	}
	async setup(request: QuotaFixtureRequest): Promise<QuotaFixtureResponse> {
		this.cached = null;
		return (await this.access()).control.request(request);
	}
	boundary(): number {
		if (this.opened === null) throw new Error('quota journal not opened');
		return this.opened.journal.read().length;
	}
	async event<TEvent>(options: {
		readonly afterSequence: number;
		readonly kind: QuotaFixtureEventKind;
		readonly signal: AbortSignal;
		readonly decode: (value: unknown) => TEvent;
	}): Promise<{ readonly sequence: number; readonly event: TEvent }> {
		const access = await this.access();
		return observeQuotaEvent({ ...access, ...options });
	}
	invalidate(): void {
		this.cached = null;
	}
	async verifyFinal(): Promise<z.infer<typeof quotaEffectReceiptSchema>> {
		const { started, journal } = await this.access();
		journal.read(true);
		const receipt = quotaEffectReceiptSchema.parse(
			JSON.parse(readFileSync(this.layout.configuration.receipt, 'utf8')),
		);
		const failures = checkQuotaEffectReceipt({
			scenario: this.layout.configuration.scenario,
			routerRoot: started.routerRoot,
			value: receipt,
		});
		if (failures.length > 0)
			throw new Error('quota final producer receipt failed: ' + failures.join('; '));
		return receipt;
	}
	async close(): Promise<void> {
		if (this.opening !== null) (await this.opening).control.close();
	}
}
