/** One S7 setup connection; acknowledgements here never impersonate native input/flush receipts. */
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
interface PendingSetup {
	readonly requestId: number;
	readonly operation: QuotaFixtureRequest['operation'];
	readonly resolve: (response: QuotaFixtureResponse) => void;
	readonly reject: (error: Error) => void;
	readonly timer: ReturnType<typeof setTimeout>;
}
export class QuotaFixtureControl {
	private readonly socket: Socket;
	private readonly lines: Interface;
	private sequence = 0;
	private pending: PendingSetup | null = null;
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
		if (this.pending !== null) {
			clearTimeout(this.pending.timer);
			this.pending.reject(this.failure);
			this.pending = null;
		}
		this.lines.close();
		this.socket.destroy();
	}
	private accept(line: string): void {
		try {
			const response = responseSchema.parse(JSON.parse(line)),
				pending = this.pending;
			if (pending === null || response.requestId !== pending.requestId)
				throw new Error('quota setup response ID differs; outcome unknown, never replay');
			const expectedKind =
				pending.operation === 'readback'
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
				response['operation'] !== pending.operation
			)
				throw new Error('quota setup response operation differs; outcome unknown, never replay');
			if ('sequence' in response || 'event' in response || 'surface' in response)
				throw new Error('quota setup response claims a native input receipt');
			clearTimeout(pending.timer);
			this.pending = null;
			if (response.kind === 'fixture-control-error')
				pending.reject(new Error('quota setup was refused: ' + String(response['message'])));
			else pending.resolve(response);
		} catch (error) {
			this.fail(error instanceof Error ? error : new Error(String(error)));
		}
	}
	request(value: QuotaFixtureRequest): Promise<QuotaFixtureResponse> {
		const request = requestSchema.parse(value);
		if (this.failure !== null) return Promise.reject(this.failure);
		if (this.pending !== null)
			return Promise.reject(new Error('quota setup allows one outstanding request; do not replay'));
		const requestId = ++this.sequence;
		return new Promise((resolve, reject) => {
			const timer = setTimeout(
				() =>
					this.fail(
						new Error('quota setup acknowledgement unavailable; outcome unknown, never replay'),
					),
				10_000,
			);
			this.pending = { requestId, operation: request.operation, resolve, reject, timer };
			this.socket.write(JSON.stringify({ ...request, requestId }) + '\n', (error) => {
				if (error !== undefined && error !== null) this.fail(error);
			});
		});
	}
	close(): void {
		this.fail(new Error('quota setup closed by its owner'));
	}
}
