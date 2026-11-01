/** One private S7 control connection; its replies never stand in for native receipts. */
import { createConnection } from 'node:net';
import type { Socket } from 'node:net';
import { createInterface } from 'node:readline';
import type { Interface } from 'node:readline';

import { z } from 'zod';

const requestSchema = z.discriminatedUnion('operation', [
	z.strictObject({ operation: z.literal('readback') }),
	z.strictObject({
		operation: z.literal('select-browse-account'),
		mount: z.number().int().positive(),
		accountId: z.string().min(1),
	}),
	z.strictObject({ operation: z.literal('reload-view-model'), mount: z.number().int().positive() }),
	z.strictObject({
		operation: z.literal('remove-captured-account-externally'),
		mount: z.number().int().positive(),
	}),
	z.strictObject({ operation: z.literal('reenable-before-removal') }),
	z.strictObject({ operation: z.literal('release-removal') }),
	z.strictObject({ operation: z.literal('release-refresh') }),
]);
export type QuotaFixtureRequest = z.infer<typeof requestSchema>;
const responseSchema = z
	.object({
		kind: z.enum([
			'fixture-readback',
			'fixture-control-ack',
			'fixture-control-error',
			'fixture-mutation',
			'fixture-gate-released',
		]),
		requestId: z.number().int().positive(),
	})
	.catchall(z.unknown());
export type QuotaFixtureResponse = z.infer<typeof responseSchema>;

// S7 event_fence.rs is the source of this closed, existing wire vocabulary.
const eventKindSchema = z.enum([
	'fixture-started',
	'view-mounted',
	'preference-save-returned',
	'grouping-invocation-started',
	'grouping-invocation-settled',
	'login-parent-exited',
	'account-runner-settled',
	'fixture-finished',
	'fixture-control-ack',
	'fixture-readback',
	'fixture-control-error',
	'fixture-mutation',
	'fixture-gate-released',
	'login-child-entered',
	'account-owner-started',
	'account-owner-finished',
	'clipboard-written',
	'removal-before-owner-held',
	'removal-after-credential-delete-held',
	'refresh-owner-started',
	'refresh-first-request-held',
	'refresh-setup-held',
]);
export type QuotaFixtureEventKind = z.infer<typeof eventKindSchema>;
const fenceRequestSchema = z.strictObject({
	operation: z.literal('await_event'),
	after_ordinal: z.number().int().nonnegative().max(Number.MAX_SAFE_INTEGER),
	kind: eventKindSchema,
});
const eventCompletionSchema = z.strictObject({
	kind: z.literal('observed'),
	ordinal: z.number().int().positive().max(Number.MAX_SAFE_INTEGER),
	eventKind: eventKindSchema,
});
export type QuotaFixtureEventCompletion = z.infer<typeof eventCompletionSchema>;
const fenceResponseSchema = z.strictObject({
	kind: z.literal('fixture-event-fence'),
	requestId: z.number().int().positive(),
	result: z.discriminatedUnion('kind', [
		eventCompletionSchema,
		z.strictObject({ kind: z.literal('ended-before-event') }),
	]),
});
const responseEnvelopeSchema = z
	.object({ kind: z.string(), requestId: z.number().int().positive() })
	.catchall(z.unknown());
type ControlRequest = QuotaFixtureRequest | z.infer<typeof fenceRequestSchema>;
interface PendingControl {
	readonly requestId: number;
	readonly operation: ControlRequest['operation'];
	readonly accept: (response: unknown) => void;
	readonly reject: (error: Error) => void;
	readonly cleanup: () => void;
}
export class QuotaFixtureControl {
	private readonly socket: Socket;
	private readonly lines: Interface;
	private sequence = 0;
	private pending: PendingControl | null = null;
	private failure: Error | null = null;
	private constructor(path: string) {
		this.socket = createConnection(path);
		this.lines = createInterface({ input: this.socket, crlfDelay: Infinity });
		this.lines.on('line', (line: string) => this.accept(line));
		this.socket.on('error', (error: Error) => this.fail(error));
		this.socket.on('close', () =>
			this.fail(
				new Error(
					'quota setup connection closed; any pending mutation has unknown outcome; never replay',
				),
			),
		);
	}
	static async connect(path: string): Promise<QuotaFixtureControl> {
		const control = new QuotaFixtureControl(path);
		await new Promise<void>((resolve, reject) => {
			control.socket.once('connect', resolve);
			control.socket.once('error', reject);
		});
		return control;
	}
	private fail(error: Error): void {
		this.failure ??= error;
		const pending = this.pending;
		this.pending = null;
		if (pending !== null) {
			pending.cleanup();
			pending.reject(this.failure);
		}
		this.lines.close();
		this.socket.destroy();
	}
	private accept(line: string): void {
		try {
			const response = responseEnvelopeSchema.parse(JSON.parse(line)),
				pending = this.pending;
			if (pending === null || response.requestId !== pending.requestId)
				throw new Error('quota setup response ID differs; outcome unknown, never replay');
			const expectedKind =
				pending.operation === 'await_event'
					? 'fixture-event-fence'
					: pending.operation === 'readback'
						? 'fixture-readback'
						: ['select-browse-account', 'reload-view-model'].includes(pending.operation)
							? 'fixture-control-ack'
							: pending.operation.startsWith('release-')
								? 'fixture-gate-released'
								: 'fixture-mutation';
			if (response.kind !== expectedKind && response.kind !== 'fixture-control-error')
				throw new Error('quota setup response kind differs; outcome unknown, never replay');
			if (
				response.kind !== 'fixture-control-error' &&
				pending.operation !== 'readback' &&
				pending.operation !== 'await_event' &&
				response['operation'] !== pending.operation
			)
				throw new Error('quota setup response operation differs; outcome unknown, never replay');
			if ('sequence' in response || 'event' in response || 'surface' in response)
				throw new Error('quota setup response claims a native input receipt');
			if (response.kind === 'fixture-control-error') {
				pending.cleanup();
				this.pending = null;
				pending.reject(new Error('quota setup was refused: ' + String(response['message'])));
			} else pending.accept(response);
		} catch (error) {
			this.fail(error instanceof Error ? error : new Error(String(error)));
		}
	}
	request(value: QuotaFixtureRequest): Promise<QuotaFixtureResponse> {
		return this.begin(requestSchema.parse(value), (response) => responseSchema.parse(response));
	}
	/** Only a post-flush wake: the caller still independently reads the real journal. */
	async awaitEvent(options: {
		readonly afterOrdinal: number;
		readonly kind: QuotaFixtureEventKind;
		readonly signal: AbortSignal;
	}): Promise<QuotaFixtureEventCompletion> {
		const request = fenceRequestSchema.parse({
			operation: 'await_event',
			after_ordinal: options.afterOrdinal,
			kind: options.kind,
		});
		options.signal.throwIfAborted();
		return this.begin(
			request,
			(response) => {
				const fence = fenceResponseSchema.parse(response);
				if (fence.result.kind === 'ended-before-event')
					throw new Error('quota fixture ended before the requested event');
				if (
					fence.result.ordinal <= request.after_ordinal ||
					fence.result.eventKind !== request.kind
				)
					throw new Error('quota event fence ordinal or kind differs from its request');
				return fence.result;
			},
			options.signal,
		);
	}
	private begin<TResult>(
		request: ControlRequest,
		validate: (response: unknown) => TResult,
		signal?: AbortSignal,
	): Promise<TResult> {
		if (this.failure !== null) return Promise.reject(this.failure);
		if (this.pending !== null)
			return Promise.reject(new Error('quota setup allows one outstanding request; do not replay'));
		const requestId = ++this.sequence;
		return new Promise((resolve, reject) => {
			// Setup keeps its existing bound; a read-only fence has only its caller's bound.
			const timer =
				signal === undefined
					? setTimeout(
							() =>
								this.fail(
									new Error(
										'quota setup acknowledgement unavailable; outcome unknown, never replay',
									),
								),
							10_000,
						)
					: null;
			const abort = (): void =>
				this.fail(
					signal?.reason instanceof Error
						? signal.reason
						: new Error('quota event fence aborted by its caller'),
				);
			const cleanup = (): void => {
				if (timer !== null) clearTimeout(timer);
				signal?.removeEventListener('abort', abort);
			};
			this.pending = {
				requestId,
				operation: request.operation,
				accept: (response) => {
					const parsed = validate(response);
					cleanup();
					this.pending = null;
					resolve(parsed);
				},
				reject,
				cleanup,
			};
			signal?.addEventListener('abort', abort, { once: true });
			if (signal?.aborted) {
				abort();
				return;
			}
			this.socket.write(JSON.stringify({ ...request, requestId }) + '\n', (error) => {
				if (error !== undefined && error !== null) this.fail(error);
			});
		});
	}
	close(): void {
		this.fail(new Error('quota setup closed by its owner'));
	}
}
